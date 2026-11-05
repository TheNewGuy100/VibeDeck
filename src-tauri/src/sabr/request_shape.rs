//! Sanitized structural description of an outgoing SABR request: query parameter
//! names, protobuf field numbers, wire types and length classes. Values are never
//! copied into the output, so a shape can be compared with a browser reference
//! without exposing tokens, signatures or identifiers.

use super::proto::{Field, Reader};
use serde::Serialize;
use std::collections::BTreeMap;

const MAX_PARAMS: usize = 64;
const MAX_HEADERS: usize = 24;
const MAX_NAME: usize = 24;
const MAX_FIELDS: usize = 256;

/// Coarse size bucket; exact lengths can fingerprint a value.
pub(crate) fn len_class(len: usize) -> &'static str {
    match len {
        0 => "0",
        1..=15 => "1-15",
        16..=255 => "16-255",
        256..=4095 => "256-4095",
        _ => ">=4096",
    }
}

/// Protocol-looking names pass through; anything else collapses to `other`.
pub(crate) fn safe_name(name: &str) -> String {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if ok {
        name.to_string()
    } else {
        "other".to_string()
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamShape {
    pub name: String,
    pub len_class: &'static str,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlShape {
    pub host_class: &'static str,
    pub path_is_videoplayback: bool,
    pub params: Vec<ParamShape>,
    pub params_truncated: bool,
}

pub(crate) fn describe_url(url: &reqwest::Url) -> UrlShape {
    let host_class = if url
        .host_str()
        .is_some_and(|h| h.ends_with(".googlevideo.com"))
    {
        "googlevideo"
    } else {
        "other"
    };
    let mut params = Vec::new();
    let mut truncated = false;
    for (name, value) in url.query_pairs() {
        if params.len() == MAX_PARAMS {
            truncated = true;
            break;
        }
        params.push(ParamShape {
            name: safe_name(&name),
            len_class: len_class(value.len()),
        });
    }
    UrlShape {
        host_class,
        path_is_videoplayback: url.path() == "/videoplayback",
        params,
        params_truncated: truncated,
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldShape {
    pub number: u32,
    pub wire: &'static str,
    pub count: usize,
    pub len_classes: Vec<&'static str>,
    pub children: Vec<FieldShape>,
    pub children_unparsed: usize,
}

/// Which length-delimited fields to parse as nested messages, by field number.
pub(crate) struct Expand {
    pub number: u32,
    pub children: &'static [Expand],
}

/// `VideoPlaybackAbrRequest`: expand the streamer context (19) and its client info (1).
pub(crate) const SABR_REQUEST_EXPAND: &[Expand] = &[Expand {
    number: 19,
    children: &[Expand {
        number: 1,
        children: &[],
    }],
}];

fn merge(into: &mut Vec<FieldShape>, from: Vec<FieldShape>) {
    for incoming in from {
        match into.iter_mut().find(|f| f.number == incoming.number) {
            Some(existing) => {
                existing.count += incoming.count;
                existing.children_unparsed += incoming.children_unparsed;
                for class in incoming.len_classes {
                    if !existing.len_classes.contains(&class) {
                        existing.len_classes.push(class);
                    }
                }
                merge(&mut existing.children, incoming.children);
            }
            None => into.push(incoming),
        }
    }
    into.sort_by_key(|f| f.number);
}

pub(crate) fn describe_message(
    bytes: &[u8],
    expand: &[Expand],
) -> Result<Vec<FieldShape>, &'static str> {
    let mut reader = Reader::new(bytes);
    let mut shapes: BTreeMap<u32, FieldShape> = BTreeMap::new();
    let mut seen = 0usize;
    while let Some((number, field)) = reader
        .next_field()
        .map_err(|_| "sabr_shape_invalid_protobuf")?
    {
        seen += 1;
        if seen > MAX_FIELDS {
            return Err("sabr_shape_too_many_fields");
        }
        let wire = match &field {
            Field::Varint(_) => "varint",
            Field::Fixed64(_) => "fixed64",
            Field::Fixed32(_) => "fixed32",
            Field::Bytes(_) => "bytes",
        };
        let entry = shapes.entry(number).or_insert_with(|| FieldShape {
            number,
            wire,
            count: 0,
            len_classes: Vec::new(),
            children: Vec::new(),
            children_unparsed: 0,
        });
        entry.count += 1;
        if let Field::Bytes(inner) = field {
            let class = len_class(inner.len());
            if !entry.len_classes.contains(&class) {
                entry.len_classes.push(class);
            }
            if let Some(spec) = expand.iter().find(|e| e.number == number) {
                match describe_message(inner, spec.children) {
                    Ok(children) => merge(&mut entry.children, children),
                    Err(_) => entry.children_unparsed += 1,
                }
            }
        }
    }
    Ok(shapes.into_values().collect())
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestShape {
    pub url: UrlShape,
    pub header_names: Vec<String>,
    pub body_len_class: &'static str,
    pub body: Vec<FieldShape>,
    pub body_parse_error: Option<&'static str>,
}

pub(crate) fn describe_request(
    url: &reqwest::Url,
    header_names: &[&str],
    body: &[u8],
) -> RequestShape {
    let mut names: Vec<String> = header_names
        .iter()
        .map(|name| safe_name(&name.to_ascii_lowercase()))
        .collect();
    names.sort();
    names.dedup();
    names.truncate(MAX_HEADERS);
    let (shape, error) = match describe_message(body, SABR_REQUEST_EXPAND) {
        Ok(shape) => (shape, None),
        Err(code) => (Vec::new(), Some(code)),
    };
    RequestShape {
        url: describe_url(url),
        header_names: names,
        body_len_class: len_class(body.len()),
        body: shape,
        body_parse_error: error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sabr::proto::Writer;

    fn sample_body(token: bool) -> Vec<u8> {
        let mut client_info = Writer::new();
        client_info.int32_field(16, 67);
        client_info.string_field(17, "1.20260101.00.00");
        let mut context = Writer::new();
        context.message_field(1, &client_info);
        if token {
            context.bytes_field(2, &[7u8; 100]);
        }
        let mut entry = Writer::new();
        entry.int32_field(1, 3);
        entry.bytes_field(2, b"ctx");
        context.message_field(5, &entry);
        context.message_field(5, &entry);
        let mut request = Writer::new();
        request.bytes_field(1, &[1, 2, 3]);
        request.bytes_field(5, &[9u8; 944]);
        request.bytes_field(16, &[4, 5]);
        request.bytes_field(19, &context.into_vec());
        request.into_vec()
    }

    #[test]
    fn describes_fields_numbers_and_length_classes_without_values() {
        let shape = describe_message(&sample_body(true), SABR_REQUEST_EXPAND).unwrap();
        let numbers: Vec<u32> = shape.iter().map(|f| f.number).collect();
        assert_eq!(numbers, vec![1, 5, 16, 19]);
        let by = |n: u32| shape.iter().find(|f| f.number == n).unwrap();
        assert_eq!(by(5).len_classes, vec!["256-4095"]);
        let ctx = by(19);
        let child = |n: u32| ctx.children.iter().find(|f| f.number == n).unwrap();
        assert_eq!(child(2).len_classes, vec!["16-255"]);
        assert_eq!(child(5).count, 2);
        let info = child(1);
        assert_eq!(
            info.children.iter().map(|f| f.number).collect::<Vec<_>>(),
            vec![16, 17]
        );
    }

    #[test]
    fn token_presence_is_visible_and_absence_is_too() {
        let with = describe_message(&sample_body(true), SABR_REQUEST_EXPAND).unwrap();
        let without = describe_message(&sample_body(false), SABR_REQUEST_EXPAND).unwrap();
        let has2 = |s: &Vec<FieldShape>| {
            s.iter()
                .find(|f| f.number == 19)
                .unwrap()
                .children
                .iter()
                .any(|f| f.number == 2)
        };
        assert!(has2(&with));
        assert!(!has2(&without));
    }

    #[test]
    fn url_shape_exports_names_and_classes_never_values() {
        let url = reqwest::Url::parse(
            "https://r1.googlevideo.com/videoplayback?sig=SECRETSIGVALUE&n=abcdefghijk&sabr=1&weird%20name=x",
        )
        .unwrap();
        let body = sample_body(true);
        let shape = describe_request(&url, &["Content-Type", "accept", "ACCEPT"], &body);
        let json = serde_json::to_string(&shape).unwrap();
        assert!(!json.contains("SECRETSIGVALUE"));
        assert!(!json.contains("abcdefghijk"));
        assert!(json.contains("\"name\":\"n\""));
        assert!(json.contains("\"name\":\"other\""));
        assert_eq!(shape.url.host_class, "googlevideo");
        assert!(shape.url.path_is_videoplayback);
        assert_eq!(shape.header_names, vec!["accept", "content-type"]);
        assert_eq!(shape.body_len_class, len_class(body.len()));
    }

    #[test]
    fn malformed_and_oversized_input_is_reported_with_fixed_codes() {
        assert_eq!(
            describe_message(&[0x08], SABR_REQUEST_EXPAND).unwrap_err(),
            "sabr_shape_invalid_protobuf"
        );
        let mut many = Writer::new();
        for _ in 0..300 {
            many.int32_field(1, 1);
        }
        assert_eq!(
            describe_message(&many.into_vec(), &[]).unwrap_err(),
            "sabr_shape_too_many_fields"
        );
        // A bad nested message is counted, not propagated.
        let mut request = Writer::new();
        request.bytes_field(19, &[0x08]);
        let shape = describe_message(&request.into_vec(), SABR_REQUEST_EXPAND).unwrap();
        assert_eq!(shape[0].children_unparsed, 1);
    }
}

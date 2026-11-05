//! Pure URL preparation only: no HTTP, cookies, logging or algorithm discovery.
//! Not wired into the transport probe; its unresolved-challenge guard stays intact.
const URL_LIMIT: usize = 64 * 1024;
const CHALLENGE_LIMIT: usize = 16 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UrlError {
    Limit,
    Destination,
    Encoding,
    Ambiguous,
    UnsupportedEnvelope,
    SignedConflict,
    Challenge,
    Transform,
}

fn validate_destination(raw: &str) -> Result<url::Url, UrlError> {
    if raw.len() > URL_LIMIT {
        return Err(UrlError::Limit);
    }
    let url = url::Url::parse(raw).map_err(|_| UrlError::Destination)?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !host.ends_with(".googlevideo.com")
        || host == ".googlevideo.com"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.path() != "/videoplayback"
        || url.fragment().is_some()
        // Fail closed if parsing normalized any byte of the input.
        || url.as_str() != raw
    {
        return Err(UrlError::Destination);
    }
    Ok(url)
}

fn decode_pair(raw: &str) -> Result<(String, String), UrlError> {
    let bytes = raw.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && (index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit())
        {
            return Err(UrlError::Encoding);
        }
    }
    let (name, value) = url::form_urlencoded::parse(bytes)
        .next()
        .ok_or(UrlError::Encoding)?;
    // form_urlencoded decoding is lossy. Reject replacement characters, including
    // legitimate U+FFFD, rather than accept ambiguous invalid UTF-8.
    if name.contains('\u{fffd}') || value.contains('\u{fffd}') {
        return Err(UrlError::Encoding);
    }
    Ok((name.into_owned(), value.into_owned()))
}

/// The callback MUST eventually come from a same-bootstrap, validated program.
/// This helper does not validate that provenance or turn a callback into approval.
/// n is independent of s/sp envelopes: those remain unsupported. rn/alr/cver/pot
/// are untouched. Only the n value is re-encoded; its original name and every other
/// raw pair (including empty pairs, ordering and duplicate unrelated names) survive.
pub(crate) fn transform_n(
    raw: &str,
    transform: impl FnOnce(&str) -> Result<String, ()>,
) -> Result<url::Url, UrlError> {
    let mut destination = validate_destination(raw)?;
    let Some(query) = destination.query() else {
        return Ok(destination);
    };
    let mut challenge = None;
    let mut signed_lists = Vec::new();
    let mut seen_lists = Vec::new();
    let mut pairs: Vec<String> = query.split('&').map(String::from).collect();
    for (index, pair) in pairs.iter().enumerate() {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = decode_pair(pair)?;
        match name.as_str() {
            "s" | "sp" | "cipher" | "signatureCipher" => {
                return Err(UrlError::UnsupportedEnvelope);
            }
            "n" => {
                if challenge.is_some() {
                    return Err(UrlError::Ambiguous);
                }
                if value.is_empty() || value.len() > CHALLENGE_LIMIT {
                    return Err(UrlError::Challenge);
                }
                challenge = Some((index, value));
            }
            "sparams" | "lsparams" => {
                if seen_lists.contains(&name) {
                    return Err(UrlError::Ambiguous);
                }
                seen_lists.push(name);
                signed_lists.push(value);
            }
            _ => {}
        }
    }
    let Some((index, input)) = challenge else {
        return Ok(destination);
    };
    if signed_lists
        .iter()
        .any(|list| list.split(',').any(|field| field == "n"))
    {
        return Err(UrlError::SignedConflict);
    }
    let output = transform(&input).map_err(|_| UrlError::Transform)?;
    if output.is_empty() || output.len() > CHALLENGE_LIMIT || output.starts_with("enhanced_except_")
    {
        return Err(UrlError::Transform);
    }
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("n", &output)
        .finish();
    let name = pairs[index].split('=').next().ok_or(UrlError::Encoding)?;
    pairs[index] = format!("{name}={}", &encoded[2..]);
    destination.set_query(Some(&pairs.join("&")));
    // Revalidate after mutation; returned URLs are never trusted implicitly.
    validate_destination(destination.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    const BASE: &str = "https://media.googlevideo.com/videoplayback";

    #[test]
    fn preserves_raw_pairs_and_encodes_only_transformed_value() {
        let raw = format!("{BASE}?sig=A%2fb+X&&%6e=a%2Bb+c&x=%26&x=two&rn=9&alr=yes&");
        let result = transform_n(&raw, |input| {
            assert_eq!(input, "a+b c");
            Ok("x&n=evil+ 🎵".into())
        })
        .unwrap();
        assert_eq!(
            result.as_str(),
            format!(
                "{BASE}?sig=A%2fb+X&&%6e=x%26n%3Devil%2B+%F0%9F%8E%B5&x=%26&x=two&rn=9&alr=yes&"
            )
        );
    }

    #[test]
    fn rejects_ambiguous_or_signed_challenges_before_callback() {
        for (query, expected) in [
            ("n=a&%6e=b", UrlError::Ambiguous),
            ("n=a&sparams=id%2Cn", UrlError::SignedConflict),
            ("n=a&lsparams=n", UrlError::SignedConflict),
            ("n=a&sparams=id&sparams=n", UrlError::Ambiguous),
            ("n=a&%73=secret", UrlError::UnsupportedEnvelope),
            ("n=a&sp=sig", UrlError::UnsupportedEnvelope),
            ("n=a&cipher=opaque", UrlError::UnsupportedEnvelope),
            ("n=", UrlError::Challenge),
            ("n=%zz", UrlError::Encoding),
            ("n=%FF", UrlError::Encoding),
        ] {
            assert_eq!(
                transform_n(&format!("{BASE}?{query}"), |_| panic!("must not invoke")),
                Err(expected)
            );
        }
    }

    #[test]
    fn no_challenge_means_no_callback_or_query_changes() {
        for suffix in ["", "?", "?x=1&&x=%2f+&"] {
            let raw = format!("{BASE}{suffix}");
            assert_eq!(
                transform_n(&raw, |_| panic!("no challenge"))
                    .unwrap()
                    .as_str(),
                raw
            );
        }
    }

    #[test]
    fn rejects_invalid_destinations_without_invoking() {
        for raw in [
            "http://media.googlevideo.com/videoplayback?n=a",
            "https://googlevideo.com/videoplayback?n=a",
            "https://media.googlevideo.com.evil.test/videoplayback?n=a",
            "https://user@media.googlevideo.com/videoplayback?n=a",
            "https://media.googlevideo.com:444/videoplayback?n=a",
            "https://media.googlevideo.com/other?n=a",
            "https://media.googlevideo.com/videoplayback?n=a#fragment",
            " https://media.googlevideo.com/videoplayback?n=a",
        ] {
            assert_eq!(
                transform_n(raw, |_| panic!("invalid destination")),
                Err(UrlError::Destination)
            );
        }
    }

    #[test]
    fn rejects_transform_errors_invalid_outputs_and_limits() {
        let raw = format!("{BASE}?n=a");
        assert_eq!(transform_n(&raw, |_| Err(())), Err(UrlError::Transform));
        for output in [
            String::new(),
            "enhanced_except_error".into(),
            "x".repeat(CHALLENGE_LIMIT + 1),
        ] {
            assert_eq!(transform_n(&raw, |_| Ok(output)), Err(UrlError::Transform));
        }
        assert_eq!(
            transform_n(
                &format!("{BASE}?n={}", "x".repeat(CHALLENGE_LIMIT + 1)),
                |_| panic!("oversize")
            ),
            Err(UrlError::Challenge)
        );
        assert_eq!(
            transform_n(&"x".repeat(URL_LIMIT + 1), |_| panic!("oversize")),
            Err(UrlError::Limit)
        );
    }
}

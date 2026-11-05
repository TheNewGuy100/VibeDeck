//! Explicit debug IPC only; no automatic invocation.
//! One POST of the byte-range UMP mode (2-byte body), taken from a signed media URL that the
//! user's own player produced and that was saved to a local file. The `pot` parameter is
//! always removed. No cookies, no Authorization. Reports classes and counts only.

use super::read_ump_varint;

const BODY_LIMIT: usize = 2 * 1024 * 1024;
static ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Default, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RangeReport {
    pub error_code: Option<&'static str>,
    pub pot_present_in_file: bool,
    pub sent: bool,
    pub status_class: Option<&'static str>,
    pub content_type_class: Option<&'static str>,
    pub response_bytes: usize,
    pub parts: std::collections::BTreeMap<u32, usize>,
    pub media_bytes: usize,
    pub protection_status: Option<u32>,
    /// Numeric (varint) fields of an error part (44), as `[field, value]`; text fields are skipped.
    pub error_fields: Vec<(u32, u64)>,
}

/// Accepts only a signed googlevideo media URL and returns it without `pot`.
fn prepare(raw: &str) -> Result<(reqwest::Url, bool), &'static str> {
    let mut url = reqwest::Url::parse(raw.trim()).map_err(|_| "range_url_invalid")?;
    let host = url.host_str().unwrap_or("");
    if url.scheme() != "https"
        || !host.ends_with(".googlevideo.com")
        || url.path() != "/videoplayback"
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("range_url_rejected");
    }
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    for needed in ["range", "sig", "expire", "itag"] {
        if !pairs.iter().any(|(k, _)| k == needed) {
            return Err("range_url_incomplete");
        }
    }
    let had_pot = pairs.iter().any(|(k, _)| k == "pot");
    let kept: Vec<(String, String)> = pairs.into_iter().filter(|(k, _)| k != "pot").collect();
    url.query_pairs_mut().clear().extend_pairs(kept);
    Ok((url, had_pot))
}

fn leb(bytes: &[u8], mut at: usize) -> Option<(u64, usize)> {
    let (mut value, mut shift) = (0u64, 0u32);
    while let Some(&b) = bytes.get(at) {
        at += 1;
        value |= u64::from(b & 0x7f).checked_shl(shift)?;
        if b & 0x80 == 0 {
            return Some((value, at));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

/// Varint fields of a flat protobuf message; length-delimited fields are skipped, never read.
fn numeric_fields(bytes: &[u8]) -> Vec<(u32, u64)> {
    let (mut out, mut at) = (Vec::new(), 0usize);
    while let Some((tag, next)) = leb(bytes, at) {
        at = next;
        match tag & 7 {
            0 => match leb(bytes, at) {
                Some((v, n)) => {
                    out.push(((tag >> 3) as u32, v));
                    at = n;
                }
                None => break,
            },
            2 => match leb(bytes, at) {
                Some((len, n)) => at = n.saturating_add(len as usize),
                None => break,
            },
            _ => break,
        }
        if out.len() >= 8 {
            break;
        }
    }
    out
}

fn error_fields_of(bytes: &[u8]) -> Vec<(u32, u64)> {
    let mut offset = 0usize;
    while let Ok(Some((id, after_id))) = read_ump_varint(bytes, offset) {
        let Ok(Some((len, start))) = read_ump_varint(bytes, after_id) else { break };
        let end = start.saturating_add(len as usize);
        if end > bytes.len() {
            break;
        }
        if id == 44 {
            return numeric_fields(&bytes[start..end]);
        }
        offset = end;
    }
    Vec::new()
}

fn describe(bytes: &[u8]) -> (std::collections::BTreeMap<u32, usize>, usize, Option<u32>) {
    let mut parts = std::collections::BTreeMap::new();
    let (mut media, mut protection, mut offset) = (0usize, None, 0usize);
    while let Ok(Some((id, after_id))) = read_ump_varint(bytes, offset) {
        let Ok(Some((len, start))) = read_ump_varint(bytes, after_id) else { break };
        let end = start.saturating_add(len as usize);
        if end > bytes.len() {
            break;
        }
        *parts.entry(id).or_insert(0) += 1;
        if id == 21 {
            media += (len as usize).saturating_sub(1);
        }
        if id == 58 && bytes.get(start) == Some(&8) {
            protection = bytes.get(start + 1).map(|b| u32::from(*b));
        }
        offset = end;
    }
    (parts, media, protection)
}

/// No call from UI/startup. One attempt per process.
#[tauri::command]
pub async fn debug_youtube_range_probe_once(
    window: tauri::WebviewWindow,
    url_file: String,
) -> Result<RangeReport, String> {
    if window.label() != "main" {
        return Err("range_wrong_window".into());
    }
    let path = std::path::PathBuf::from(&url_file);
    if path.file_name().and_then(|n| n.to_str()) != Some("range-url.txt") {
        return Err("range_file_rejected".into());
    }
    if ATTEMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("range_already_attempted".into());
    }
    let raw = std::fs::read_to_string(&path).map_err(|_| "range_file_unreadable")?;
    let (url, had_pot) = prepare(&raw)?;
    send(url, had_pot).await
}

fn request(http: &reqwest::Client, url: reqwest::Url) -> reqwest::RequestBuilder {
    http.post(url)
        .header("Origin", "https://music.youtube.com")
        .header("Referer", "https://music.youtube.com/")
        .header("Accept", "*/*")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        )
        .body(vec![0x78u8, 0x00])
}

/// One byte-range POST returning the whole UMP body; only 2xx with a UMP content type succeeds.
pub(super) async fn fetch_body(
    http: &reqwest::Client,
    url: reqwest::Url,
) -> Result<Vec<u8>, &'static str> {
    let response = request(http, url)
        .send()
        .await
        .map_err(|e| super::super::youtube_http::request_error(&e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(super::transport_probe::status_error(status));
    }
    if super::transport_probe::content_type_class(response.headers()) != "ump" {
        return Err("range_body_not_ump");
    }
    super::super::youtube_http::bounded_body(response, BODY_LIMIT)
        .await
        .map_err(|_| "range_body_failed")
}

/// One byte-range POST (2-byte body, no cookies) to a prepared googlevideo URL.
pub(super) async fn send(url: reqwest::Url, had_pot: bool) -> Result<RangeReport, String> {
    let mut report = RangeReport {
        pot_present_in_file: had_pot,
        ..Default::default()
    };
    let http = super::super::youtube_http::client()?;
    report.sent = true;
    let response = request(&http, url).send().await;
    let response = match response {
        Ok(r) => r,
        Err(e) => {
            report.error_code = Some(super::super::youtube_http::request_error(&e));
            return Ok(report);
        }
    };
    report.status_class = Some(super::transport_probe::status_class(response.status()));
    report.content_type_class = Some(super::transport_probe::content_type_class(response.headers()));
    match super::super::youtube_http::bounded_body(response, BODY_LIMIT).await {
        Ok(bytes) => {
            report.response_bytes = bytes.len();
            let (parts, media, protection) = describe(&bytes);
            report.parts = parts;
            report.media_bytes = media;
            report.protection_status = protection;
            report.error_fields = error_fields_of(&bytes);
        }
        Err(_) => report.error_code = Some("range_body_failed"),
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "https://rr1---sn-x.googlevideo.com/videoplayback?expire=1&itag=140&range=0-9&sig=AAA&pot=SECRET&n=abc";

    #[test]
    fn pot_is_always_removed_and_other_pairs_survive() {
        let (url, had) = prepare(GOOD).unwrap();
        assert!(had);
        let query = url.query().unwrap();
        assert!(!query.contains("pot") && !query.contains("SECRET"));
        for kept in ["expire=1", "itag=140", "range=0-9", "sig=AAA", "n=abc"] {
            assert!(query.contains(kept));
        }
    }

    #[test]
    fn foreign_or_incomplete_urls_are_refused() {
        assert_eq!(prepare("http://a.googlevideo.com/videoplayback?range=1&sig=1&expire=1&itag=1").unwrap_err(), "range_url_rejected");
        assert_eq!(prepare("https://evil.example/videoplayback?range=1&sig=1&expire=1&itag=1").unwrap_err(), "range_url_rejected");
        assert_eq!(prepare("https://a.googlevideo.com/videoplayback?range=1").unwrap_err(), "range_url_incomplete");
    }

    #[test]
    fn error_part_exposes_only_numeric_fields() {
        // part 44, length 9: field 1 string "ab", field 2 varint 7, field 3 varint 300
        let bytes = [44u8, 9, 0x0a, 2, b'a', b'b', 0x10, 7, 0x18, 0xac, 0x02];
        assert_eq!(error_fields_of(&bytes), vec![(2, 7), (3, 300)]);
        assert!(error_fields_of(&[21, 1, 0]).is_empty());
    }

    #[test]
    fn describe_counts_parts_media_and_protection() {
        // part 20 (len 1), part 21 (len 4: 1 id byte + 3 media), part 58 (len 2: tag 8, value 1)
        let bytes = [20u8, 1, 0, 21, 4, 0, 1, 2, 3, 58, 2, 8, 1];
        let (parts, media, protection) = describe(&bytes);
        assert_eq!(parts.get(&21), Some(&1));
        assert_eq!(media, 3);
        assert_eq!(protection, Some(1));
    }
}

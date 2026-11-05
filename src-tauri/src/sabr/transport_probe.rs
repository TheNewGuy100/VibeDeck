//! Explicit debug IPC only; no automatic invocation.
//! One bounded POST, no credentials from the YouTube cookie jar, no audio output.

use super::request_shape::{describe_request, describe_url, safe_name, RequestShape, UrlShape};
use super::{build_abr_request, read_ump_varint, AbrRequestState, SabrBootstrap};

const BODY_LIMIT: usize = 4 * 1024 * 1024;
const REQUEST_LIMIT: usize = 256 * 1024;
const PART_LIMIT: usize = 4096;

static ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBudget {
    auxiliary_get: u8,
    player_post: u8,
    sabr_post: u8,
}

impl RequestBudget {
    /// The SABR POST count when the bootstrap budget is exactly one GET and one player POST.
    pub(super) fn sabr_posts(&self) -> Option<u8> {
        (self.auxiliary_get == 1 && self.player_post == 1).then_some(self.sabr_post)
    }
}

fn check_budget(budget: &RequestBudget) -> Result<(), &'static str> {
    check_budget_for(budget, false)
}

/// A dry run still needs the player bootstrap (1 GET + 1 player POST) but sends no SABR POST.
fn check_budget_for(budget: &RequestBudget, dry_run: bool) -> Result<(), &'static str> {
    if *budget
        != (RequestBudget {
            auxiliary_get: 1,
            player_post: 1,
            sabr_post: u8::from(!dry_run),
        })
    {
        return Err("sabr_probe_budget_rejected");
    }
    Ok(())
}

/// No call from UI/startup. Approval must explicitly cover this request budget.
/// One attempt per process, including failures; no reset endpoint or retry queue.
#[tauri::command]
pub async fn debug_youtube_sabr_transport_once(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: RequestBudget,
    // Experiment only: send the `n` challenge value exactly as received. `s` still blocks.
    allow_raw_n: Option<bool>,
    // Builds the exact request and reports its sanitized shape, but sends no SABR POST.
    dry_run: Option<bool>,
    // Experiment: send the request the way the web player was observed to (URL, body, headers).
    mirror_browser: Option<bool>,
    // Experiment: transform the `n` challenge with the cached player and the ejs solver (about a
    // minute). Needs the native-playback-trial feature, the solver in LOCALAPPDATA and a fresh cache.
    solve_n: Option<bool>,
) -> Result<TransportReport, String> {
    if window.label() != "main" {
        return Err("sabr_probe_wrong_window".into());
    }
    let dry_run = dry_run.unwrap_or(false);
    check_budget_for(&budget, dry_run)?;
    if ATTEMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("sabr_probe_already_attempted".into());
    }
    let bootstrap = load_bootstrap(&app).await?;
    let options = ProbeOptions {
        allow_raw_n: allow_raw_n.unwrap_or(false),
        dry_run,
        mirror_browser: mirror_browser.unwrap_or(false),
        solve_n: solve_n.unwrap_or(false),
    };
    Ok(run_once_with(bootstrap, options).await)
}

/// Player bootstrap for the first YouTube track in the local catalog. Shared by the debug probes.
pub(super) async fn load_bootstrap(app: &tauri::AppHandle) -> Result<SabrBootstrap, &'static str> {
    let catalog = super::super::playlist_catalog::load_playlist_catalog(app.clone())
        .map_err(|_| "sabr_probe_catalog_unavailable")?;
    let video_id = catalog
        .playlists
        .iter()
        .flat_map(|playlist| playlist.tracks.iter())
        .filter(|track| track.get("source").and_then(|value| value.as_str()) == Some("youtube"))
        .filter_map(|track| track.get("youtubeId").and_then(|value| value.as_str()))
        .find(|id| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
        .ok_or("sabr_probe_catalog_track_missing")?;
    let fetch = super::super::fetch_youtube_player(app, video_id)
        .await
        .map_err(|_| "sabr_probe_bootstrap_fetch_failed")?;
    let bootstrap =
        SabrBootstrap::from_player_payload(&fetch.payload, video_id, &fetch.client_version)
            .map_err(|_| "sabr_probe_bootstrap_rejected")?;
    Ok(bootstrap)
}

#[derive(Clone, Copy, Default)]
pub struct ProbeOptions {
    pub allow_raw_n: bool,
    pub dry_run: bool,
    pub mirror_browser: bool,
    pub solve_n: bool,
}

/// Safe projection only. Unknown part IDs and protection values are not exported.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportReport {
    pub response_bytes: usize,
    pub part_count: usize,
    pub media_parts: usize,
    /// Includes the UMP media header-ID byte; not decoded audio bytes.
    pub media_payload_bytes: usize,
    pub initialization_parts: usize,
    pub redirect_requested: bool,
    pub reload_requested: bool,
    pub error_present: bool,
    pub protection_parts: usize,
    pub protection_status: Option<u8>,
    pub unknown_protection_status: bool,
    pub unknown_parts: usize,
    /// True only when the experiment flag was set and the URL carried an `n` that was sent untransformed.
    pub raw_n_sent: bool,
    /// Fixed code of the first failure after the bootstrap; the rest of the report stays useful.
    pub error_code: Option<&'static str>,
    pub dry_run: bool,
    /// True when the `n` sent was produced by the solver instead of the received value.
    pub n_solved: bool,
    /// True when URL, body and headers followed the observed web player format.
    pub mirrored: bool,
    /// True once the SABR POST was handed to the HTTP client.
    pub sent: bool,
    /// `2xx`, `3xx`, `401_403`, `429`, `4xx`, `5xx` or `other`; never the number.
    pub status_class: Option<&'static str>,
    pub content_type_class: Option<&'static str>,
    pub response_header_names: Vec<String>,
    /// Names and length classes only, even when the guard refuses the URL.
    pub url_shape: Option<UrlShape>,
    pub request_shape: Option<RequestShape>,
}

impl TransportReport {
    /// Copies the response summary fields without touching the request-side projection.
    fn absorb(&mut self, summary: TransportReport) {
        self.response_bytes = summary.response_bytes;
        self.part_count = summary.part_count;
        self.media_parts = summary.media_parts;
        self.media_payload_bytes = summary.media_payload_bytes;
        self.initialization_parts = summary.initialization_parts;
        self.redirect_requested = summary.redirect_requested;
        self.reload_requested = summary.reload_requested;
        self.error_present = summary.error_present;
        self.protection_parts = summary.protection_parts;
        self.protection_status = summary.protection_status;
        self.unknown_protection_status = summary.unknown_protection_status;
        self.unknown_parts = summary.unknown_parts;
    }
}

pub(super) fn status_class(status: reqwest::StatusCode) -> &'static str {
    match status.as_u16() {
        200..=299 => "2xx",
        300..=399 => "3xx",
        401 | 403 => "401_403",
        429 => "429",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    }
}

pub(super) fn content_type_class(headers: &reqwest::header::HeaderMap) -> &'static str {
    let value = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if value.is_empty() {
        "none"
    } else if value.contains("vnd.yt-ump") {
        "ump"
    } else if value.contains("protobuf") {
        "protobuf"
    } else if value.contains("html") {
        "html"
    } else if value.contains("json") {
        "json"
    } else if value.starts_with("text/") {
        "text"
    } else {
        "other"
    }
}

fn response_header_names(headers: &reqwest::header::HeaderMap) -> Vec<String> {
    let mut names: Vec<String> = headers.keys().map(|k| safe_name(k.as_str())).collect();
    names.sort();
    names.dedup();
    names.truncate(24);
    names
}

fn prepare_url(raw: &str) -> Result<reqwest::Url, &'static str> {
    prepare_url_with(raw, false)
}

pub(super) fn prepare_url_with(raw: &str, allow_raw_n: bool) -> Result<reqwest::Url, &'static str> {
    let mut url = reqwest::Url::parse(raw).map_err(|_| "sabr_probe_invalid_url")?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !host.ends_with(".googlevideo.com")
        || host == ".googlevideo.com"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.path() != "/videoplayback"
        || url.fragment().is_some()
    {
        return Err("sabr_probe_invalid_url");
    }
    // Do not silently attempt decipher/n transformation. The caller must review
    // the bootstrap first; retaining an n challenge is not equivalent to solving it.
    // The code names which challenge parameters are present (names only, never values).
    let has_s = url.query_pairs().any(|(name, _)| name == "s");
    let has_n = url.query_pairs().any(|(name, _)| name == "n");
    if has_s || (has_n && !allow_raw_n) {
        return Err(match (has_s, has_n) {
            (true, false) => "sabr_probe_url_transform_required_s",
            (false, true) => "sabr_probe_url_transform_required_n",
            _ => "sabr_probe_url_transform_required_s_n",
        });
    }
    if url.query_pairs().any(|(name, value)| {
        matches!(name.as_ref(), "sparams" | "lsparams")
            && value.split(',').any(|field| matches!(field, "rn" | "alr"))
    }) {
        return Err("sabr_probe_signed_query_conflict");
    }
    // Work on raw pairs, not decoded values: percent-escape case, encoded
    // separators, plus signs, duplicates and ordering of signed data must survive.
    let mut pairs = Vec::new();
    if let Some(query) = url.query() {
        for raw_pair in query.split('&') {
            let name = url::form_urlencoded::parse(raw_pair.as_bytes())
                .next()
                .map(|(name, _)| name.into_owned())
                .unwrap_or_default();
            if name != "rn" && name != "alr" {
                pairs.push(raw_pair);
            }
        }
    }
    let mut query = pairs.join("&");
    if !query.is_empty() {
        query.push('&');
    }
    query.push_str("rn=0&alr=no");
    url.set_query(Some(&query));
    Ok(url)
}

/// Replaces the value of the `n` pair in the raw query, keeping every other pair byte for byte.
pub(super) fn replace_n(raw: &str, solved: &str) -> Option<String> {
    let (base, query) = raw.split_once('?')?;
    let mut found = false;
    let pairs: Vec<String> = query
        .split('&')
        .map(|pair| {
            if pair.split('=').next() == Some("n") {
                found = true;
                format!("n={solved}")
            } else {
                pair.to_string()
            }
        })
        .collect();
    found.then(|| format!("{base}?{}", pairs.join("&")))
}

/// Web player query: adds `cpn` (kept when present) and `cver`, and `alr=yes`.
fn mirror_url(url: &mut reqwest::Url, client_version: &str) {
    mirror_url_with(url, client_version, None, 0);
}

/// Same, with a session-wide `cpn` (kept from the URL or generated when `None`) and the request counter.
pub(super) fn mirror_url_with(
    url: &mut reqwest::Url,
    client_version: &str,
    cpn: Option<&str>,
    rn: u32,
) -> String {
    let cpn = cpn
        .map(str::to_string)
        .or_else(|| {
            url.query_pairs()
                .find(|(name, _)| name == "cpn")
                .map(|(_, value)| value.into_owned())
        })
        .unwrap_or_else(new_cpn);
    let mut pairs: Vec<String> = Vec::new();
    if let Some(query) = url.query() {
        for raw_pair in query.split('&') {
            let name = raw_pair.split('=').next().unwrap_or_default();
            if !matches!(name, "rn" | "alr" | "cpn" | "cver") {
                pairs.push(raw_pair.to_string());
            }
        }
    }
    let cver: String = url::form_urlencoded::byte_serialize(client_version.as_bytes()).collect();
    pairs.push(format!("cpn={cpn}"));
    pairs.push(format!("cver={cver}"));
    pairs.push(format!("rn={rn}"));
    pairs.push("alr=yes".to_string());
    url.set_query(Some(&pairs.join("&")));
    cpn
}

/// 16 characters from the URL-safe base64 alphabet; a client nonce, not a secret.
pub(super) fn new_cpn() -> String {
    use std::hash::{BuildHasher, Hasher};
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    (0..16)
        .map(|index| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_usize(index);
            ALPHABET[(hasher.finish() % 64) as usize] as char
        })
        .collect()
}

#[cfg(feature = "native-playback-trial")]
pub(super) fn solve_n_blocking(challenge: &str) -> Result<String, &'static str> {
    use vibedeck_decipher_eval::{ejs_solver, player_cache};
    let local = std::env::var_os("LOCALAPPDATA").ok_or("sabr_probe_solver_unavailable")?;
    let solver_dir = std::path::PathBuf::from(local)
        .join("VibeDeck")
        .join("ejs-solver");
    let project = std::env::current_dir().unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let base =
        player_cache::default_base(&project).map_err(|_| "sabr_probe_player_cache_unavailable")?;
    let (_, player) =
        player_cache::load(&base, None, now).map_err(|_| "sabr_probe_player_cache_empty")?;
    ejs_solver::solve_one(&solver_dir, &player, challenge).map_err(|_| "sabr_probe_n_solver_failed")
}

#[cfg(not(feature = "native-playback-trial"))]
pub(super) fn solve_n_blocking(_challenge: &str) -> Result<String, &'static str> {
    Err("sabr_probe_solver_unavailable")
}

/// Fixed category only; the numeric status and response body are never exported.
pub(super) fn status_error(status: reqwest::StatusCode) -> &'static str {
    match status.as_u16() {
        300..=399 => "sabr_probe_http_redirect",
        401 | 403 => "sabr_probe_http_access_denied",
        429 => "sabr_probe_http_rate_limited",
        400..=499 => "sabr_probe_http_client_status",
        500..=599 => "sabr_probe_http_server_status",
        _ => "sabr_probe_http_unexpected_status",
    }
}

fn summarize(body: &[u8]) -> Result<TransportReport, &'static str> {
    if body.is_empty() {
        return Err("sabr_probe_empty_body");
    }
    if body.len() > BODY_LIMIT {
        return Err("sabr_probe_body_too_large");
    }
    let mut report = TransportReport {
        response_bytes: body.len(),
        ..Default::default()
    };
    let mut offset = 0;
    while offset < body.len() {
        if report.part_count == PART_LIMIT {
            return Err("sabr_probe_too_many_parts");
        }
        let (kind, next) = read_ump_varint(body, offset)
            .map_err(|_| "sabr_probe_invalid_ump")?
            .ok_or("sabr_probe_truncated_ump")?;
        let (size, start) = read_ump_varint(body, next)
            .map_err(|_| "sabr_probe_invalid_ump")?
            .ok_or("sabr_probe_truncated_ump")?;
        let end = start
            .checked_add(size as usize)
            .ok_or("sabr_probe_invalid_ump")?;
        let payload = body.get(start..end).ok_or("sabr_probe_truncated_ump")?;
        offset = end;
        report.part_count += 1;
        match kind {
            21 => {
                if payload.is_empty() {
                    return Err("sabr_probe_invalid_media");
                }
                report.media_parts += 1;
                report.media_payload_bytes += payload.len();
            }
            42 => report.initialization_parts += 1,
            43 => report.redirect_requested = true,
            44 => report.error_present = true,
            46 => report.reload_requested = true,
            58 => {
                report.protection_parts += 1;
                let mut reader = super::proto::Reader::new(payload);
                while let Some((field, value)) = reader
                    .next_field()
                    .map_err(|_| "sabr_probe_invalid_protection")?
                {
                    if field == 1 {
                        match value {
                            super::proto::Field::Varint(status @ 0..=3) => {
                                report.protection_status = Some(status as u8)
                            }
                            super::proto::Field::Varint(_) => {
                                report.unknown_protection_status = true
                            }
                            _ => return Err("sabr_probe_invalid_protection"),
                        }
                    }
                }
            }
            20 | 22 | 35 | 45 | 57 | 59 | 62 => {}
            _ => report.unknown_parts += 1,
        }
    }
    Ok(report)
}

/// Requires a separately authorized, counted invocation by a future debug caller.
/// Consumes the bootstrap in memory. Never reloads player or follows UMP controls.
///
/// Never fails as a whole: the first error becomes `error_code` and everything observed
/// up to that point (URL shape, request shape, status class, header names) stays in the
/// report, so a counted attempt always teaches as much as possible.
pub async fn run_once(
    bootstrap: SabrBootstrap,
    allow_raw_n: bool,
    dry_run: bool,
) -> TransportReport {
    run_once_with(
        bootstrap,
        ProbeOptions {
            allow_raw_n,
            dry_run,
            ..Default::default()
        },
    )
    .await
}

pub async fn run_once_with(bootstrap: SabrBootstrap, options: ProbeOptions) -> TransportReport {
    let mut report = TransportReport {
        dry_run: options.dry_run,
        mirrored: options.mirror_browser,
        ..Default::default()
    };
    if let Err(code) = execute(bootstrap, options, &mut report).await {
        report.error_code = Some(code);
    }
    report
}

async fn execute(
    mut bootstrap: SabrBootstrap,
    options: ProbeOptions,
    report: &mut TransportReport,
) -> Result<(), &'static str> {
    let ProbeOptions {
        allow_raw_n,
        dry_run,
        mirror_browser,
        solve_n,
    } = options;
    // Names and size classes of the real URL, even if the guard refuses it below.
    if let Ok(parsed) = reqwest::Url::parse(&bootstrap.streaming_url) {
        report.url_shape = Some(describe_url(&parsed));
    }
    let mut raw_url = bootstrap.streaming_url.clone();
    if solve_n {
        let challenge = reqwest::Url::parse(&raw_url)
            .ok()
            .and_then(|parsed| {
                parsed
                    .query_pairs()
                    .find(|(name, _)| name == "n")
                    .map(|(_, value)| value.into_owned())
            })
            .ok_or("sabr_probe_n_missing")?;
        let solved = tauri::async_runtime::spawn_blocking(move || solve_n_blocking(&challenge))
            .await
            .map_err(|_| "sabr_probe_solver_failed")??;
        raw_url = replace_n(&raw_url, &solved).ok_or("sabr_probe_n_missing")?;
        report.n_solved = true;
    }
    let mut url = prepare_url_with(&raw_url, allow_raw_n || report.n_solved)?;
    report.raw_n_sent =
        allow_raw_n && !report.n_solved && url.query_pairs().any(|(name, _)| name == "n");
    let mut state = AbrRequestState::new(0);
    if mirror_browser {
        mirror_url(&mut url, &bootstrap.client_version);
        // The web player reads its token from a client-side minter, not from the player
        // response; this experiment sends none, which is the question being asked.
        bootstrap.po_token = None;
        state.browser_shape = true;
    }
    let request = build_abr_request(&bootstrap, &state);
    if request.len() > REQUEST_LIMIT {
        return Err("sabr_probe_request_too_large");
    }
    let header_names: &[&str] = if mirror_browser {
        &[]
    } else {
        &["content-type", "accept", "accept-encoding"]
    };
    report.request_shape = Some(describe_request(&url, header_names, &request));
    if dry_run {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "sabr_probe_client_failed")?;
    report.sent = true;
    let mut builder = client.post(url);
    if !mirror_browser {
        builder = builder
            .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
            .header(reqwest::header::ACCEPT, "application/vnd.yt-ump")
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
    }
    let response = builder
        .body(request)
        .send()
        .await
        .map_err(|error| super::super::youtube_http::request_error(&error))?;
    report.status_class = Some(status_class(response.status()));
    report.content_type_class = Some(content_type_class(response.headers()));
    report.response_header_names = response_header_names(response.headers());
    if !response.status().is_success() {
        return Err(status_error(response.status()));
    }
    if report.content_type_class != Some("ump") {
        return Err("sabr_probe_unexpected_content_type");
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .is_some_and(|value| value != "identity")
    {
        return Err("sabr_probe_unexpected_encoding");
    }
    let body = super::super::youtube_http::bounded_body(response, BODY_LIMIT)
        .await
        .map_err(|_| "sabr_probe_body_failed")?;
    report.absorb(summarize(&body)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_bootstrap(query: &str) -> SabrBootstrap {
        let payload = serde_json::json!({
            "playabilityStatus": { "status": "OK" },
            "videoDetails": { "lengthSeconds": "231" },
            "playerConfig": { "mediaCommonConfig": { "mediaUstreamerRequestConfig": {
                "videoPlaybackUstreamerConfig": "AQID" } } },
            "streamingData": {
                "serverAbrStreamingUrl":
                    format!("https://rr1---sn.example.googlevideo.com/videoplayback?{query}"),
                "adaptiveFormats": [{
                    "itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000,
                    "audioQuality": "AUDIO_QUALITY_HIGH", "sampleRate": "48000",
                    "audioChannels": 2, "lastModified": "1700000000000001"
                }]
            }
        });
        SabrBootstrap::from_player_payload(&payload, "abc", "1.2026").unwrap()
    }

    #[test]
    fn dry_run_budget_needs_bootstrap_requests_but_no_sabr_post() {
        let dry = RequestBudget {
            auxiliary_get: 1,
            player_post: 1,
            sabr_post: 0,
        };
        assert!(check_budget_for(&dry, true).is_ok());
        assert!(check_budget_for(&dry, false).is_err());
        let full = RequestBudget {
            auxiliary_get: 1,
            player_post: 1,
            sabr_post: 1,
        };
        assert!(check_budget_for(&full, true).is_err());
    }

    #[test]
    fn dry_run_reports_request_shape_without_sending() {
        let bootstrap = synthetic_bootstrap("sabr=1&n=abc123&cpn=zzz");
        let report = tauri::async_runtime::block_on(run_once(bootstrap, true, true));
        assert_eq!(report.error_code, None);
        assert!(report.dry_run && !report.sent && report.raw_n_sent);
        let shape = report.request_shape.as_ref().unwrap();
        let names: Vec<&str> = shape.url.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["sabr", "n", "cpn", "rn", "alr"]);
        assert!(shape.body.iter().any(|f| f.number == 19));
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("abc123") && !json.contains("zzz"));
    }

    #[test]
    fn refused_url_still_reports_param_names_and_a_fixed_code() {
        let n = synthetic_bootstrap("sabr=1&n=abc123");
        let report = tauri::async_runtime::block_on(run_once(n, false, true));
        assert_eq!(
            report.error_code,
            Some("sabr_probe_url_transform_required_n")
        );
        assert!(report.request_shape.is_none());
        let names: Vec<&str> = report
            .url_shape
            .as_ref()
            .unwrap()
            .params
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, vec!["sabr", "n"]);
        let s = synthetic_bootstrap("s=abc&n=def");
        let report = tauri::async_runtime::block_on(run_once(s, true, true));
        assert_eq!(
            report.error_code,
            Some("sabr_probe_url_transform_required_s_n")
        );
    }

    #[test]
    fn response_projection_helpers_export_classes_only() {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
        let status = |c: u16| status_class(reqwest::StatusCode::from_u16(c).unwrap());
        assert_eq!(
            [
                status(200),
                status(302),
                status(401),
                status(403),
                status(429),
                status(404),
                status(503),
                status(100)
            ],
            ["2xx", "3xx", "401_403", "401_403", "429", "4xx", "5xx", "other"]
        );
        let mut headers = HeaderMap::new();
        assert_eq!(content_type_class(&headers), "none");
        for (value, expected) in [
            ("application/vnd.yt-ump", "ump"),
            ("application/x-protobuf", "protobuf"),
            ("text/html; charset=utf-8", "html"),
            ("application/json", "json"),
            ("text/plain", "text"),
            ("image/png", "other"),
        ] {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static(value));
            assert_eq!(content_type_class(&headers), expected);
        }
        headers.insert(
            "X-Secret-Marker",
            HeaderValue::from_static("value-not-exported"),
        );
        let names = response_header_names(&headers);
        assert!(names.contains(&"x-secret-marker".to_string()));
        assert!(!serde_json::to_string(&names)
            .unwrap()
            .contains("value-not-exported"));
    }

    #[test]
    fn budget_is_exact_and_rejects_extra_fields() {
        let budget = RequestBudget {
            auxiliary_get: 1,
            player_post: 1,
            sabr_post: 1,
        };
        assert!(check_budget(&budget).is_ok());
        for budget in [
            RequestBudget {
                auxiliary_get: 0,
                player_post: 1,
                sabr_post: 1,
            },
            RequestBudget {
                auxiliary_get: 1,
                player_post: 2,
                sabr_post: 1,
            },
            RequestBudget {
                auxiliary_get: 1,
                player_post: 1,
                sabr_post: 2,
            },
        ] {
            assert_eq!(
                check_budget(&budget).unwrap_err(),
                "sabr_probe_budget_rejected"
            );
        }
        assert!(serde_json::from_str::<RequestBudget>(
            r#"{"auxiliaryGet":1,"playerPost":1,"sabrPost":1,"retry":true}"#
        )
        .is_err());
    }

    #[test]
    fn rejects_untrusted_destinations_and_unresolved_challenges() {
        for raw in [
            "http://r.googlevideo.com/videoplayback",
            "https://googlevideo.com/videoplayback",
            "https://r.googlevideo.com.evil.invalid/videoplayback",
            "https://user@r.googlevideo.com/videoplayback",
            "https://r.googlevideo.com:444/videoplayback",
            "https://r.googlevideo.com/other",
            "https://127.0.0.1/videoplayback",
            "https://r.googlevideo.com/videoplayback#secret",
        ] {
            assert!(matches!(prepare_url(raw), Err("sabr_probe_invalid_url")));
        }
        assert!(matches!(
            prepare_url("https://r.googlevideo.com/videoplayback?n=synthetic"),
            Err("sabr_probe_url_transform_required_n")
        ));
        assert!(matches!(
            prepare_url("https://r.googlevideo.com/videoplayback?s=synthetic&sp=sig"),
            Err("sabr_probe_url_transform_required_s")
        ));
        assert!(matches!(
            prepare_url("https://r.googlevideo.com/videoplayback?n=synthetic&s=synthetic"),
            Err("sabr_probe_url_transform_required_s_n")
        ));
        let url =
            prepare_url("https://r.googlevideo.com/videoplayback?sig=synthetic&rn=9&rn=8&alr=yes")
                .unwrap();
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(pairs.iter().filter(|(name, _)| name == "rn").count(), 1);
        assert!(pairs
            .iter()
            .any(|(name, value)| name == "rn" && value == "0"));
        assert!(pairs
            .iter()
            .any(|(name, value)| name == "alr" && value == "no"));
        assert!(pairs
            .iter()
            .any(|(name, value)| name == "sig" && value == "synthetic"));
    }

    #[test]
    fn http_status_is_projected_as_fixed_category_only() {
        let cases = [
            (302, "sabr_probe_http_redirect"),
            (401, "sabr_probe_http_access_denied"),
            (403, "sabr_probe_http_access_denied"),
            (429, "sabr_probe_http_rate_limited"),
            (400, "sabr_probe_http_client_status"),
            (404, "sabr_probe_http_client_status"),
            (500, "sabr_probe_http_server_status"),
            (503, "sabr_probe_http_server_status"),
            (100, "sabr_probe_http_unexpected_status"),
        ];
        for (code, expected) in cases {
            let status = reqwest::StatusCode::from_u16(code).unwrap();
            assert_eq!(status_error(status), expected, "{code}");
            assert!(!status_error(status).contains(&code.to_string()));
        }
    }

    #[test]
    fn raw_n_is_opt_in_and_preserves_bytes_but_never_allows_s() {
        let base = "https://r.googlevideo.com/videoplayback";
        assert!(matches!(
            prepare_url_with(&format!("{base}?n=a%2Fb&x=1"), false),
            Err("sabr_probe_url_transform_required_n")
        ));
        let url = prepare_url_with(&format!("{base}?x=1&n=a%2Fb%2Bc&y=2"), true).unwrap();
        assert_eq!(url.query().unwrap(), "x=1&n=a%2Fb%2Bc&y=2&rn=0&alr=no");
        assert!(matches!(
            prepare_url_with(&format!("{base}?n=a&s=b"), true),
            Err("sabr_probe_url_transform_required_s_n")
        ));
        assert!(matches!(
            prepare_url_with(&format!("{base}?s=b"), true),
            Err("sabr_probe_url_transform_required_s")
        ));
    }

    #[test]
    fn signed_query_bytes_are_preserved_or_rejected() {
        let raw = "sig=A%2fb%2Bc+Z&key=a%26rn%3D9&key=second&bare&empty=&sparams=sig%2Ckey";
        let url = prepare_url(&format!(
            "https://r.googlevideo.com/videoplayback?{raw}&%72n=5&al%72=yes"
        ))
        .unwrap();
        assert_eq!(url.query().unwrap(), format!("{raw}&rn=0&alr=no"));
        for query in ["sparams=sig%2Crn", "lsparams=alr", "%73params=alr%2Csig"] {
            assert!(matches!(
                prepare_url(&format!("https://r.googlevideo.com/videoplayback?{query}")),
                Err("sabr_probe_signed_query_conflict")
            ));
        }
        assert!(matches!(
            prepare_url("https://r.googlevideo.com/videoplayback?%6e=synthetic"),
            Err("sabr_probe_url_transform_required_n")
        ));
    }

    #[test]
    fn framing_is_strict_and_resource_bounded() {
        for body in [&[20][..], &[20, 2, 0][..], &[0x80][..]] {
            assert_eq!(summarize(body).unwrap_err(), "sabr_probe_truncated_ump");
        }
        assert_eq!(summarize(&[0xff]).unwrap_err(), "sabr_probe_invalid_ump");
        assert_eq!(summarize(&[]).unwrap_err(), "sabr_probe_empty_body");
        assert_eq!(summarize(&[21, 0]).unwrap_err(), "sabr_probe_invalid_media");
        assert_eq!(
            summarize(&vec![0; BODY_LIMIT + 1]).unwrap_err(),
            "sabr_probe_body_too_large"
        );
        assert_eq!(
            summarize(&[20, 0].repeat(PART_LIMIT + 1)).unwrap_err(),
            "sabr_probe_too_many_parts"
        );
    }

    #[test]
    fn projection_does_not_export_control_payloads() {
        let report = summarize(&[
            42, 0, 21, 3, 1, 99, 98, 43, 2, 99, 98, 46, 0, 44, 0, 58, 2, 8, 2, 58, 2, 8, 99, 99, 0,
        ])
        .unwrap();
        assert_eq!(report.part_count, 8);
        assert_eq!(report.media_payload_bytes, 3);
        assert_eq!(report.protection_status, Some(2));
        assert!(report.unknown_protection_status);
        assert!(report.redirect_requested && report.reload_requested && report.error_present);
        assert_eq!(report.unknown_parts, 1);
        assert_eq!(
            summarize(&[58, 1, 8]).unwrap_err(),
            "sabr_probe_invalid_protection"
        );
    }

    #[test]
    fn replace_n_changes_only_the_n_pair() {
        let raw = "https://rr1.googlevideo.com/videoplayback?a=1&n=OLDVALUE1&sig=A%2Bb&z=9";
        assert_eq!(
            replace_n(raw, "NEWVALUE").unwrap(),
            "https://rr1.googlevideo.com/videoplayback?a=1&n=NEWVALUE&sig=A%2Bb&z=9"
        );
        assert!(replace_n("https://rr1.googlevideo.com/videoplayback?a=1", "X").is_none());
        // A parameter that merely starts with n must not be touched.
        assert!(replace_n("https://h/videoplayback?nn=1", "X").is_none());
    }

    #[test]
    fn mirror_url_adds_cpn_cver_and_keeps_an_existing_cpn() {
        let mut url = reqwest::Url::parse(
            "https://rr1.googlevideo.com/videoplayback?sabr=1&n=X&rn=0&alr=no&sig=A%2Bb",
        )
        .unwrap();
        mirror_url(&mut url, "1.20261014.01.00");
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect();
        let names: Vec<&str> = pairs.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["sabr", "n", "sig", "cpn", "cver", "rn", "alr"]);
        let value = |name: &str| pairs.iter().find(|(n, _)| n == name).unwrap().1.clone();
        assert_eq!(value("cver"), "1.20261014.01.00");
        assert_eq!(value("alr"), "yes");
        assert_eq!(value("sig"), "A+b");
        let cpn = value("cpn");
        assert_eq!(cpn.len(), 16);
        assert!(cpn
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));

        let mut kept = reqwest::Url::parse(
            "https://rr1.googlevideo.com/videoplayback?cpn=ABCDEFGHIJKLMNOP&n=X",
        )
        .unwrap();
        mirror_url(&mut kept, "1");
        assert_eq!(
            kept.query_pairs()
                .find(|(name, _)| name == "cpn")
                .unwrap()
                .1,
            "ABCDEFGHIJKLMNOP"
        );
    }

    #[test]
    fn mirrored_dry_run_reports_the_web_player_shape_without_sending() {
        let bootstrap = synthetic_bootstrap("sabr=1&sig=S");
        let report = tauri::async_runtime::block_on(run_once_with(
            bootstrap,
            ProbeOptions {
                dry_run: true,
                mirror_browser: true,
                ..Default::default()
            },
        ));
        assert!(report.mirrored && report.dry_run && !report.sent);
        assert_eq!(report.error_code, None);
        let request = report.request_shape.unwrap();
        assert!(request.header_names.is_empty());
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("cpn") && json.contains("cver"));
    }
}

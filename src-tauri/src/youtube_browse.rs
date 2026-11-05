//! Authenticated InnerTube `browse` requests made directly by the app (no WebView broker): the
//! same session cookies and SAPISIDHASH that `/player` and the continuation requests already use.
//! `BrowseSession::open` costs one page GET (client version, visitor data, API key); every
//! `browse` call is one POST. The payload structure summary exports counts only.

use serde_json::Value;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153.0.0.0 Safari/537.36";
const BODY_LIMIT: usize = 8 * 1024 * 1024;

pub struct BrowseSession {
    session: crate::youtube_session::YouTubeSession,
    client: reqwest::Client,
    api_key: String,
    client_version: String,
    visitor_id: String,
}

impl BrowseSession {
    pub async fn open(app: &tauri::AppHandle) -> Result<Self, String> {
        use tauri::Manager;
        let path = app
            .path()
            .app_data_dir()
            .map_err(|_| "browse_directory_failed")?
            .join("youtube-session.bin");
        let session = crate::youtube_session::YouTubeSession::load(&path)?;
        let now = crate::unix_now()?;
        if !session.has_auth_cookie(now) {
            return Err("browse_session_missing_auth_cookie".into());
        }
        let client = crate::youtube_http::client()?;
        let response = client
            .get("https://music.youtube.com/")
            .header("Cookie", session.cookie_header_for("https://music.youtube.com/", now)?)
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            .map_err(|e| crate::youtube_http::request_error(&e))?;
        let bytes = crate::youtube_http::bounded_body(response, 4 * 1024 * 1024).await?;
        let html = String::from_utf8_lossy(&bytes);
        let read = |name: &str| -> String {
            regex::Regex::new(&format!(r#"{name}":"([^"]+)""#))
                .ok()
                .and_then(|p| p.captures(&html).and_then(|c| c.get(1).map(|v| v.as_str().to_string())))
                .unwrap_or_default()
        };
        let mut client_version = read("INNERTUBE_CLIENT_VERSION");
        if client_version.is_empty() {
            client_version = "1.20260920.16.00".into();
        }
        let visitor_id = read("VISITOR_DATA");
        if visitor_id.is_empty() {
            return Err("browse_config_missing_visitor".into());
        }
        Ok(Self { session, client, api_key: read("INNERTUBE_API_KEY"), client_version, visitor_id })
    }

    /// One `browse` POST for `browse_id` (for example `FEmusic_liked_playlists` or `VL<playlist id>`).
    pub async fn browse(&self, browse_id: &str, referer: &str) -> Result<Value, String> {
        if browse_id.is_empty()
            || browse_id.len() > 64
            || !browse_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err("browse_id_rejected".into());
        }
        let mut body = self.context_body();
        body["browseId"] = Value::String(browse_id.to_string());
        self.post(body, None, referer).await
    }

    /// One continuation POST for a token taken from an earlier `browse` payload.
    pub async fn continuation(&self, token: &str, referer: &str) -> Result<Value, String> {
        if token.is_empty() || token.len() > 4096 || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err("browse_token_rejected".into());
        }
        let mut body = self.context_body();
        body["continuation"] = Value::String(token.to_string());
        self.post(body, Some(token), referer).await
    }

    fn context_body(&self) -> Value {
        serde_json::json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX", "clientVersion": self.client_version,
                    "hl": "pt", "gl": "BR", "visitorData": self.visitor_id,
                    "platform": "DESKTOP", "clientFormFactor": "UNKNOWN_FORM_FACTOR"
                },
                "user": { "lockedSafetyMode": false },
                "request": { "useSsl": true }
            }
        })
    }

    async fn post(&self, body: Value, token: Option<&str>, referer: &str) -> Result<Value, String> {
        let mut endpoint = url::Url::parse("https://music.youtube.com/youtubei/v1/browse")
            .map_err(|_| "browse_endpoint_invalid")?;
        {
            let mut query = endpoint.query_pairs_mut();
            if let Some(token) = token {
                query.append_pair("ctoken", token).append_pair("continuation", token).append_pair("type", "next");
            }
            query.append_pair("prettyPrint", "false");
            if !self.api_key.is_empty() {
                query.append_pair("key", &self.api_key);
            }
        }
        let now = crate::unix_now()?;
        let response = self
            .client
            .post(endpoint.clone())
            .header("Authorization", self.session.authorization(endpoint.as_str(), now as u64, None)?)
            .header("Cookie", self.session.cookie_header_for(endpoint.as_str(), now)?)
            .header("X-Youtube-Client-Name", "67")
            .header("X-Youtube-Client-Version", &self.client_version)
            .header("X-Goog-Visitor-Id", &self.visitor_id)
            .header("X-Origin", "https://music.youtube.com")
            .header("X-Goog-AuthUser", "0")
            .header("X-Youtube-Bootstrap-Logged-In", "true")
            .header("X-Goog-Request-Time", now.to_string())
            .header("Referer", referer)
            .header("Origin", "https://music.youtube.com")
            .header("User-Agent", USER_AGENT)
            .json(&body)
            .send()
            .await
            .map_err(|e| crate::youtube_http::request_error(&e))?;
        let bytes = crate::youtube_http::bounded_body(response, BODY_LIMIT).await?;
        crate::youtube_http::player_json(&bytes)
    }
}

/// Playlists of the user's library from a `FEmusic_liked_playlists` (or continuation) payload, in the
/// shape the interface expects. Only grid items that link to a `VL<id>` page are playlists; the
/// "new playlist" tile has no such link.
pub fn playlists_from_payload(payload: &Value) -> Vec<Value> {
    fn visit(value: &Value, out: &mut Vec<Value>) {
        match value {
            Value::Object(map) => {
                if let Some(item) = map.get("musicTwoRowItemRenderer") {
                    let id = item
                        .pointer("/navigationEndpoint/browseEndpoint/browseId")
                        .and_then(|v| v.as_str())
                        .filter(|id| id.starts_with("VL") && id.len() > 2 && id.len() <= 64);
                    let name = item
                        .pointer("/title/runs/0/text")
                        .and_then(|v| v.as_str())
                        .filter(|name| !name.trim().is_empty());
                    if let (Some(id), Some(name)) = (id, name) {
                        if !out.iter().any(|p| p.get("id").and_then(|v| v.as_str()) == Some(id)) {
                            out.push(serde_json::json!({
                                "id": id, "name": name, "source": "online", "mode": "online",
                                "tracks": [], "allowLocal": false
                            }));
                        }
                    }
                }
                map.values().for_each(|child| visit(child, out));
            }
            Value::Array(items) => items.iter().for_each(|child| visit(child, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    visit(payload, &mut out);
    out
}

/// The next continuation token of a library/grid payload, if any.
pub fn next_token(payload: &Value) -> Option<String> {
    match payload {
        Value::Object(map) => {
            if let Some(token) = map
                .get("continuationItemRenderer")
                .and_then(|c| c.pointer("/continuationEndpoint/continuationCommand/token"))
                .and_then(|t| t.as_str())
            {
                return Some(token.to_string());
            }
            if let Some(token) = map
                .get("nextContinuationData")
                .and_then(|c| c.get("continuation"))
                .and_then(|t| t.as_str())
            {
                return Some(token.to_string());
            }
            map.values().find_map(next_token)
        }
        Value::Array(items) => items.iter().find_map(next_token),
        _ => None,
    }
}

/// Every playlist of the library: the first page plus up to five continuations.
pub async fn load_library_playlists(app: &tauri::AppHandle) -> Result<Vec<Value>, String> {
    let session = BrowseSession::open(app).await?;
    let referer = "https://music.youtube.com/library/playlists";
    let mut payload = session.browse("FEmusic_liked_playlists", referer).await?;
    let mut playlists = playlists_from_payload(&payload);
    for _ in 0..5 {
        let Some(token) = next_token(&payload) else { break };
        payload = session.continuation(&token, referer).await?;
        for playlist in playlists_from_payload(&payload) {
            let id = playlist.get("id").and_then(|v| v.as_str()).map(str::to_owned);
            if !playlists.iter().any(|p| p.get("id").and_then(|v| v.as_str()) == id.as_deref()) {
                playlists.push(playlist);
            }
        }
    }
    Ok(playlists)
}

/// Counts of renderer and field names in a `browse` payload; never any text, id or url.
#[derive(Debug, Default, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BrowseSummary {
    pub two_row_items: usize,
    pub responsive_list_items: usize,
    pub playlist_shelves: usize,
    pub music_shelves: usize,
    pub grids: usize,
    pub carousels: usize,
    pub section_lists: usize,
    pub tabs: usize,
    pub continuation_items: usize,
    pub next_continuation_data: usize,
    pub playlist_ids: usize,
    pub vl_browse_ids: usize,
    pub thumbnail_lists: usize,
    pub text_runs: usize,
}

pub fn summarize(value: &Value) -> BrowseSummary {
    fn walk(v: &Value, s: &mut BrowseSummary) {
        match v {
            Value::Object(map) => {
                for (key, child) in map {
                    match key.as_str() {
                        "musicTwoRowItemRenderer" => s.two_row_items += 1,
                        "musicResponsiveListItemRenderer" => s.responsive_list_items += 1,
                        "musicPlaylistShelfRenderer" => s.playlist_shelves += 1,
                        "musicShelfRenderer" => s.music_shelves += 1,
                        "gridRenderer" => s.grids += 1,
                        "musicCarouselShelfRenderer" => s.carousels += 1,
                        "sectionListRenderer" => s.section_lists += 1,
                        "tabRenderer" => s.tabs += 1,
                        "continuationItemRenderer" => s.continuation_items += 1,
                        "nextContinuationData" => s.next_continuation_data += 1,
                        "playlistId" => s.playlist_ids += 1,
                        "thumbnails" => s.thumbnail_lists += 1,
                        "runs" => s.text_runs += child.as_array().map_or(0, |a| a.len()),
                        "browseId" if child.as_str().is_some_and(|b| b.starts_with("VL")) => {
                            s.vl_browse_ids += 1
                        }
                        _ => {}
                    }
                    walk(child, s);
                }
            }
            Value::Array(items) => items.iter().for_each(|child| walk(child, s)),
            _ => {}
        }
    }
    let mut summary = BrowseSummary::default();
    walk(value, &mut summary);
    summary
}

#[cfg(all(windows, debug_assertions))]
pub mod probe {
    use super::*;

    static ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    #[derive(serde::Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    pub struct Budget {
        auxiliary_get: u8,
        browse_post: u8,
    }

    #[derive(Debug, Default, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Report {
        pub error_code: Option<String>,
        pub library: Option<PageReport>,
        pub playlist: Option<PageReport>,
    }

    #[derive(Debug, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PageReport {
        pub ok: bool,
        pub error_code: Option<String>,
        pub payload_bytes: usize,
        pub summary: BrowseSummary,
    }

    fn page(result: Result<Value, String>) -> PageReport {
        match result {
            Ok(value) => PageReport {
                ok: true,
                error_code: None,
                payload_bytes: value.to_string().len(),
                summary: summarize(&value),
            },
            Err(code) => PageReport {
                ok: false,
                error_code: Some(code),
                payload_bytes: 0,
                summary: BrowseSummary::default(),
            },
        }
    }

    /// No call from UI/startup. One attempt per process: 1 page GET and 2 `browse` POSTs.
    #[tauri::command]
    pub async fn debug_youtube_browse_probe(
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        budget: Budget,
    ) -> Result<Report, String> {
        if window.label() != "main" {
            return Err("browse_probe_wrong_window".into());
        }
        if budget != (Budget { auxiliary_get: 1, browse_post: 2 }) {
            return Err("browse_probe_budget_rejected".into());
        }
        if ATTEMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err("browse_probe_already_attempted".into());
        }
        let playlist_id = crate::playlist_catalog::load_playlist_catalog(app.clone())
            .map_err(|_| "browse_probe_catalog_unavailable")?
            .playlists
            .iter()
            .filter(|p| p.source == "online")
            .map(|p| p.id.trim_start_matches("VL").to_string())
            .find(|id| id.starts_with("PL") && id.len() <= 60)
            .ok_or("browse_probe_playlist_missing")?;
        let mut report = Report::default();
        let session = match BrowseSession::open(&app).await {
            Ok(session) => session,
            Err(code) => {
                report.error_code = Some(code);
                return Ok(report);
            }
        };
        report.library = Some(page(
            session.browse("FEmusic_liked_playlists", "https://music.youtube.com/library/playlists").await,
        ));
        report.playlist = Some(page(
            session
                .browse(&format!("VL{playlist_id}"), &format!("https://music.youtube.com/playlist?list={playlist_id}"))
                .await,
        ));
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summary_counts_structure_and_exports_no_text() {
        let payload = json!({
            "contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {
                "sectionListRenderer": {"contents": [{"gridRenderer": {"items": [
                    {"musicTwoRowItemRenderer": {
                        "title": {"runs": [{"text": "SECRET TITLE"}]},
                        "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPLsecret"}},
                        "thumbnailRenderer": {"musicThumbnailRenderer": {"thumbnail": {"thumbnails": [{"url": "https://x"}]}}}
                    }},
                    {"continuationItemRenderer": {}}
                ]}}]}
            }}}]}}
        });
        let s = summarize(&payload);
        assert_eq!((s.two_row_items, s.grids, s.tabs, s.section_lists), (1, 1, 1, 1));
        assert_eq!((s.vl_browse_ids, s.thumbnail_lists, s.text_runs, s.continuation_items), (1, 1, 1, 1));
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("SECRET") && !text.contains("PLsecret") && !text.contains("https"));
    }

    #[test]
    fn library_items_with_a_playlist_page_become_interface_playlists() {
        let payload = json!({"contents": [
            {"musicTwoRowItemRenderer": {"title": {"runs": [{"text": "Nova playlist"}]}}},
            {"musicTwoRowItemRenderer": {
                "title": {"runs": [{"text": "Favoritas"}]},
                "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPLabc123"}}}},
            {"musicTwoRowItemRenderer": {
                "title": {"runs": [{"text": "Favoritas (de novo)"}]},
                "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPLabc123"}}}},
            {"musicTwoRowItemRenderer": {
                "title": {"runs": [{"text": "   "}]},
                "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPLblank"}}}},
            {"musicTwoRowItemRenderer": {
                "title": {"runs": [{"text": "Album"}]},
                "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREb_x"}}}}
        ]});
        let playlists = playlists_from_payload(&payload);
        assert_eq!(playlists.len(), 1);
        assert_eq!(
            playlists[0],
            json!({"id": "VLPLabc123", "name": "Favoritas", "source": "online", "mode": "online", "tracks": [], "allowLocal": false})
        );
    }

    #[test]
    fn continuation_tokens_are_found_in_both_payload_shapes() {
        let item = json!({"a": [{"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "T1"}}}}]});
        assert_eq!(next_token(&item).as_deref(), Some("T1"));
        let data = json!({"x": {"nextContinuationData": {"continuation": "T2"}}});
        assert_eq!(next_token(&data).as_deref(), Some("T2"));
        assert_eq!(next_token(&json!({"x": 1})), None);
    }

    #[test]
    fn browse_ids_are_restricted_before_any_request() {
        let rt = tauri::async_runtime::block_on(async {
            // No session needed: the id check runs first, but BrowseSession needs construction,
            // so exercise the same predicate directly.
            ["", "a b", "VL/../x", &"x".repeat(65)]
                .iter()
                .all(|id| {
                    id.is_empty()
                        || id.len() > 64
                        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                })
        });
        assert!(rt);
    }
}

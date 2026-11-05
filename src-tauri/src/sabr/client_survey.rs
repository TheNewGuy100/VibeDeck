//! Explicit debug IPC only; no automatic invocation.
//! Asks `/player` for the same track as other official client types and reports only the
//! structure of the answer (counts and booleans). No cookies, no Authorization, no SABR POST,
//! no media. One survey per process; the budget is the number of clients (at most two).

use serde_json::{json, Value};

const BODY_LIMIT: usize = 4 * 1024 * 1024;
static ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Client {
    AndroidVr,
    Tv,
}

impl Client {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "android_vr" => Some(Self::AndroidVr),
            "tv" => Some(Self::Tv),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::AndroidVr => "android_vr",
            Self::Tv => "tv",
        }
    }

    fn id_header(self) -> &'static str {
        match self {
            Self::AndroidVr => "28",
            Self::Tv => "7",
        }
    }

    fn version(self) -> &'static str {
        match self {
            Self::AndroidVr => "1.60.19",
            Self::Tv => "7.20250219.14.00",
        }
    }

    fn user_agent(self) -> &'static str {
        match self {
            Self::AndroidVr => "com.google.android.apps.youtube.vr.oculus/1.60.19 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
            Self::Tv => "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version",
        }
    }

    fn body(self, video: &str) -> Value {
        let client = match self {
            Self::AndroidVr => json!({
                "clientName": "ANDROID_VR", "clientVersion": self.version(),
                "deviceMake": "Oculus", "deviceModel": "Quest 3", "osName": "Android",
                "osVersion": "12L", "androidSdkVersion": 32, "hl": "en", "gl": "US"
            }),
            Self::Tv => json!({
                "clientName": "TVHTML5", "clientVersion": self.version(), "hl": "en", "gl": "US"
            }),
        };
        json!({
            "context": { "client": client, "user": { "lockedSafetyMode": false }, "request": { "useSsl": true } },
            "videoId": video, "contentCheckOk": true, "racyCheckOk": true
        })
    }
}

/// Structure only: never URLs, ids, tokens or text from the provider.
#[derive(Debug, Default, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ClientReport {
    pub client: &'static str,
    pub error_code: Option<&'static str>,
    pub playability: &'static str,
    pub muxed_formats: usize,
    pub adaptive_formats: usize,
    pub audio_formats: usize,
    pub audio_with_direct_url: usize,
    pub audio_with_cipher: usize,
    pub audio_without_url: usize,
    pub has_server_abr_url: bool,
    pub has_ustreamer_config: bool,
    pub has_hls_manifest: bool,
    pub direct_url_has_pot: bool,
    pub direct_url_has_n: bool,
}

fn project(client: Client, payload: &Value) -> ClientReport {
    let streaming = payload.get("streamingData");
    let list = |key: &str| -> Vec<&Value> {
        streaming
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().collect())
            .unwrap_or_default()
    };
    let muxed = list("formats");
    let adaptive = list("adaptiveFormats");
    let is_audio = |f: &&Value| {
        f.get("mimeType")
            .and_then(|m| m.as_str())
            .is_some_and(|m| m.starts_with("audio/"))
    };
    let audio: Vec<&Value> = adaptive.iter().copied().filter(|f| is_audio(f)).collect();
    let direct: Vec<&Value> = audio.iter().copied().filter(|f| f.get("url").is_some()).collect();
    let query_has = |name: &str| {
        direct.iter().any(|f| {
            f.get("url")
                .and_then(|u| u.as_str())
                .and_then(|u| url::Url::parse(u).ok())
                .is_some_and(|u| u.query_pairs().any(|(k, _)| k == name))
        })
    };
    ClientReport {
        client: client.label(),
        error_code: None,
        playability: super::super::youtube_http::playability_status(payload),
        muxed_formats: muxed.len(),
        adaptive_formats: adaptive.len(),
        audio_formats: audio.len(),
        audio_with_direct_url: direct.len(),
        audio_with_cipher: audio
            .iter()
            .filter(|f| f.get("signatureCipher").is_some() || f.get("cipher").is_some())
            .count(),
        audio_without_url: audio
            .iter()
            .filter(|f| {
                f.get("url").is_none()
                    && f.get("signatureCipher").is_none()
                    && f.get("cipher").is_none()
            })
            .count(),
        has_server_abr_url: streaming.is_some_and(|s| s.get("serverAbrStreamingUrl").is_some()),
        has_ustreamer_config: payload
            .pointer("/playerConfig/mediaCommonConfig/mediaUstreamerRequestConfig/videoPlaybackUstreamerConfig")
            .is_some(),
        has_hls_manifest: streaming.is_some_and(|s| s.get("hlsManifestUrl").is_some()),
        direct_url_has_pot: query_has("pot"),
        direct_url_has_n: query_has("n"),
    }
}

pub(super) fn first_catalog_video(app: &tauri::AppHandle) -> Result<String, &'static str> {
    let catalog = super::super::playlist_catalog::load_playlist_catalog(app.clone())
        .map_err(|_| "survey_catalog_unavailable")?;
    catalog
        .playlists
        .iter()
        .flat_map(|p| p.tracks.iter())
        .filter(|t| t.get("source").and_then(|v| v.as_str()) == Some("youtube"))
        .filter_map(|t| t.get("youtubeId").and_then(|v| v.as_str()))
        .find(|id| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .map(str::to_owned)
        .ok_or("survey_catalog_track_missing")
}

/// The account session of the app: cookie header and SAPISIDHASH for the music origin.
struct Account {
    cookie: String,
    authorization: String,
}

fn load_account(app: &tauri::AppHandle) -> Result<Account, &'static str> {
    use tauri::Manager;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| "survey_directory_failed")?
        .join("youtube-session.bin");
    let session = super::super::youtube_session::YouTubeSession::load(&path)
        .map_err(|_| "survey_session_unavailable")?;
    let url = "https://music.youtube.com/youtubei/v1/player";
    let now = super::super::unix_now().map_err(|_| "survey_clock_failed")?;
    Ok(Account {
        cookie: session
            .cookie_header_for(url, now)
            .map_err(|_| "survey_session_ineligible")?,
        authorization: session
            .authorization(url, now as u64, None)
            .map_err(|_| "survey_session_ineligible")?,
    })
}

async fn ask(
    http: &reqwest::Client,
    client: Client,
    video: &str,
    account: Option<&Account>,
) -> ClientReport {
    let fail = |code: &'static str| ClientReport {
        client: client.label(),
        error_code: Some(code),
        playability: "unknown",
        ..Default::default()
    };
    let mut request = match account {
        None => http.post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false"),
        Some(account) => http
            .post("https://music.youtube.com/youtubei/v1/player?prettyPrint=false")
            .header("Cookie", &account.cookie)
            .header("Authorization", &account.authorization)
            .header("X-Origin", "https://music.youtube.com")
            .header("Origin", "https://music.youtube.com")
            .header("X-Goog-AuthUser", "0")
            .header("X-Youtube-Bootstrap-Logged-In", "true"),
    };
    request = request
        .header("X-Youtube-Client-Name", client.id_header())
        .header("X-Youtube-Client-Version", client.version())
        .header("User-Agent", client.user_agent())
        .json(&client.body(video));
    let response = request.send().await;
    let response = match response {
        Ok(r) => r,
        Err(e) => return fail(super::super::youtube_http::request_error(&e)),
    };
    let bytes = match super::super::youtube_http::bounded_body(response, BODY_LIMIT).await {
        Ok(b) => b,
        Err(_) => return fail("survey_http_failed"),
    };
    match super::super::youtube_http::player_json(&bytes) {
        Ok(payload) => project(client, &payload),
        Err(_) => fail("survey_payload_rejected"),
    }
}

/// No call from UI/startup. One attempt per process; `clients` is a subset of
/// `android_vr` and `tv` and its length is the number of `/player` POSTs sent.
#[tauri::command]
pub async fn debug_youtube_player_client_survey(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    clients: Vec<String>,
    // Only with exactly `["tv"]`: sends the app's own account session (cookie + SAPISIDHASH).
    authenticated: Option<bool>,
) -> Result<Vec<ClientReport>, String> {
    if window.label() != "main" {
        return Err("survey_wrong_window".into());
    }
    let mut parsed: Vec<Client> = Vec::new();
    for name in &clients {
        let client = Client::parse(name).ok_or("survey_client_rejected")?;
        if parsed.contains(&client) {
            return Err("survey_client_duplicated".into());
        }
        parsed.push(client);
    }
    if parsed.is_empty() || parsed.len() > 2 {
        return Err("survey_budget_rejected".into());
    }
    let authenticated = authenticated.unwrap_or(false);
    if authenticated && parsed != [Client::Tv] {
        return Err("survey_authenticated_tv_only".into());
    }
    if ATTEMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("survey_already_attempted".into());
    }
    let video = first_catalog_video(&app)?;
    let account = if authenticated {
        Some(load_account(&app)?)
    } else {
        None
    };
    let http = super::super::youtube_http::client()?;
    let mut reports = Vec::new();
    for client in parsed {
        reports.push(ask(&http, client, &video, account.as_ref()).await);
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_counts_structure_and_never_exports_values() {
        let payload = json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {
                "formats": [{"url": "https://x/y"}],
                "adaptiveFormats": [
                    {"mimeType": "audio/webm; codecs=\"opus\"", "url": "https://r.example/p?n=abc&pot=zzz"},
                    {"mimeType": "audio/mp4", "signatureCipher": "s=1"},
                    {"mimeType": "audio/mp4"},
                    {"mimeType": "video/mp4", "url": "https://v.example/p"}
                ],
                "serverAbrStreamingUrl": "https://s.example/abr"
            }
        });
        let report = project(Client::AndroidVr, &payload);
        assert_eq!(report.playability, "OK");
        assert_eq!(
            (report.muxed_formats, report.adaptive_formats, report.audio_formats),
            (1, 4, 3)
        );
        assert_eq!(
            (report.audio_with_direct_url, report.audio_with_cipher, report.audio_without_url),
            (1, 1, 1)
        );
        assert!(report.has_server_abr_url && !report.has_ustreamer_config && !report.has_hls_manifest);
        assert!(report.direct_url_has_pot && report.direct_url_has_n);
        let text = serde_json::to_string(&report).unwrap();
        assert!(!text.contains("example") && !text.contains("zzz") && !text.contains("abc"));
    }

    #[test]
    fn empty_payload_projects_zeros() {
        let report = project(Client::Tv, &json!({}));
        assert_eq!(report.playability, "unknown");
        assert_eq!(report.adaptive_formats, 0);
        assert!(!report.has_server_abr_url);
    }

    #[test]
    fn bodies_carry_no_credentials_and_distinct_client_names() {
        let a = Client::AndroidVr.body("abcdefghijk").to_string();
        let t = Client::Tv.body("abcdefghijk").to_string();
        assert!(a.contains("ANDROID_VR") && t.contains("TVHTML5"));
        assert!(!a.contains("visitorData") && !t.contains("visitorData"));
    }
}

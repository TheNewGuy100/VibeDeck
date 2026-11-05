#[cfg(windows)]
use std::collections::{HashMap, HashSet};
#[cfg(windows)]
use std::io::{Read, Write};
#[cfg(windows)]
use std::net::TcpListener;
#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use youtube_session::{unix_now, YouTubeSession};

#[cfg(windows)]
use tauri::{Emitter, Manager};

#[cfg(windows)]
use windows_sys::Win32::Foundation::LocalFree;

#[cfg(windows)]
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

#[cfg(windows)]
use windows_core::Interface;

mod audio;
mod local_library;
mod playlist_catalog;
#[cfg(windows)]
mod sabr;
#[cfg(all(windows, feature = "native-range-playback"))]
mod native_range;
#[cfg(all(windows, feature = "native-range-playback"))]
mod native_range_session;
#[cfg(windows)]
mod youtube_cookie_bridge;
#[cfg(windows)]
mod youtube_http;
#[cfg(windows)]
mod youtube_browse;
#[cfg(windows)]
mod youtube_session;
#[cfg(all(windows, debug_assertions))]
mod youtube_session_debug;

#[cfg(windows)]
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualizerControlsPreference {
    open: bool,
    revision: u64,
}

#[cfg(windows)]
static VISUALIZER_CONTROLS_PREFERENCE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(windows)]
fn visualizer_controls_preferences_path(
    app: &tauri::AppHandle,
) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("settings")
        .join("visualizer-controls.json"))
}

#[cfg(windows)]
#[tauri::command]
fn get_visualizer_controls_preference(
    app: tauri::AppHandle,
) -> Result<VisualizerControlsPreference, String> {
    let path = visualizer_controls_preferences_path(&app)?;
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(VisualizerControlsPreference {
                open: true,
                revision: 0,
            })
        }
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(windows)]
#[tauri::command]
fn set_visualizer_controls_preference(
    app: tauri::AppHandle,
    open: bool,
    revision: u64,
) -> Result<VisualizerControlsPreference, String> {
    let _guard = VISUALIZER_CONTROLS_PREFERENCE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|error| error.to_string())?;
    let path = visualizer_controls_preferences_path(&app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid settings path".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let current = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<VisualizerControlsPreference>(&bytes).ok());
    if let Some(current) = current {
        if revision <= current.revision {
            return Ok(current);
        }
    }
    let preference = VisualizerControlsPreference { open, revision };
    let temp = path.with_extension(format!("{}.tmp", revision));
    std::fs::write(
        &temp,
        serde_json::to_vec(&preference).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(temp, path).map_err(|error| error.to_string())?;
    Ok(preference)
}

#[cfg(windows)]
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerpentsSnakePreference {
    id: String,
    lane: String,
    x: f64,
    z: f64,
    scale: f64,
    phase: f64,
}

#[cfg(windows)]
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerpentsLightPreference {
    id: String,
    x: f64,
    y: f64,
    z: f64,
}

#[cfg(windows)]
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SerpentsPreference {
    version: u8,
    revision: u64,
    snakes: Vec<SerpentsSnakePreference>,
    lights: Vec<SerpentsLightPreference>,
}

#[cfg(windows)]
static SERPENTS_PREFERENCE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(windows)]
fn serpents_preferences_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("settings")
        .join("serpents-preferences.json"))
}

#[cfg(windows)]
#[tauri::command]
fn get_serpents_preference(app: tauri::AppHandle) -> Result<SerpentsPreference, String> {
    match std::fs::read(serpents_preferences_path(&app)?) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SerpentsPreference {
            version: 1,
            revision: 0,
            snakes: Vec::new(),
            lights: Vec::new(),
        }),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(windows)]
#[tauri::command]
fn set_serpents_preference(
    app: tauri::AppHandle,
    preference: SerpentsPreference,
) -> Result<SerpentsPreference, String> {
    let _guard = SERPENTS_PREFERENCE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|error| error.to_string())?;
    if preference.version != 1
        || !(2..=8).contains(&preference.snakes.len())
        || preference.lights.is_empty()
        || preference.lights.len() > 8
        || preference.snakes.iter().any(|snake| {
            !["left", "right", "center"].contains(&snake.lane.as_str())
                || !(-9.0..=9.0).contains(&snake.x)
                || !(-6.0..=3.0).contains(&snake.z)
                || !(0.5..=1.4).contains(&snake.scale)
                || !(0.0..=6.284).contains(&snake.phase)
        })
        || preference.lights.iter().any(|light| {
            !(-12.0..=12.0).contains(&light.x)
                || !(-9.0..=9.0).contains(&light.y)
                || !(-10.0..=14.0).contains(&light.z)
        })
    {
        return Err("Serpents preference payload is outside supported bounds".to_string());
    }
    let path = serpents_preferences_path(&app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Invalid settings path".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let current = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SerpentsPreference>(&bytes).ok());
    if let Some(current) = current {
        if preference.revision <= current.revision {
            return Ok(current);
        }
    }
    let temp = path.with_extension(format!("{}.tmp", preference.revision));
    std::fs::write(
        &temp,
        serde_json::to_vec(&preference).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(temp, path).map_err(|error| error.to_string())?;
    Ok(preference)
}

#[cfg(windows)]
#[cfg(windows)]
fn protect_data(secret: Vec<u8>, unprotect: bool) -> Result<Vec<u8>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: secret.len() as u32,
        pbData: secret.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    let success = unsafe {
        if unprotect {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if success == 0 || output.pbData.is_null() {
        return Err("Windows DPAPI failed to protect the secret".to_string());
    }
    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as *mut core::ffi::c_void);
    }
    Ok(protected)
}

#[cfg(windows)]
#[tauri::command]
fn protect_secret(secret: Vec<u8>) -> Result<Vec<u8>, String> {
    protect_data(secret, false)
}

#[cfg(windows)]
#[tauri::command]
fn unprotect_secret(secret: Vec<u8>) -> Result<Vec<u8>, String> {
    protect_data(secret, true)
}

#[cfg(windows)]
static OAUTH_CALLBACK: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_LIBRARY_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_LOGIN_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_STREAM_BRIDGE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(windows)]
static UMP_RELAY_PAGE_ENDED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static UMP_RELAY_PAGE_ERROR: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
static APP_LOGS: OnceLock<Mutex<Vec<AppLogEntry>>> = OnceLock::new();

#[cfg(windows)]
static APP_LOG_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(windows)]
static YOUTUBE_PLAYLIST_DEBUG: OnceLock<Mutex<PlaylistBridgeDebugState>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_PLAYLIST_CONTINUATIONS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_PLAYLIST_SEEN_CONTINUATIONS: OnceLock<Mutex<HashMap<String, HashSet<String>>>> =
    OnceLock::new();

#[cfg(windows)]
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppLogEntry {
    id: u64,
    timestamp: u64,
    level: &'static str,
    source: &'static str,
    window: Option<&'static str>,
    operation: String,
    message: String,
    code: Option<String>,
    correlation_id: Option<String>,
    details: Option<serde_json::Value>,
}

#[cfg(windows)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppLogRequest {
    message: String,
    operation: String,
    correlation_id: String,
    level: Option<String>,
    code: Option<String>,
    details: Option<serde_json::Value>,
}

#[cfg(windows)]
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistBridgeDebugState {
    playlist_id: Option<String>,
    request_id: Option<String>,
    stage: String,
    initial_tracks: usize,
    last_page_tracks: usize,
    total_tracks: usize,
}

#[cfg(windows)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct YouTubePlaylistPage {
    tracks: Vec<serde_json::Value>,
    has_more: bool,
    continuation: Option<String>,
}

#[cfg(windows)]
fn playlist_continuation_state(playlist_id: &str) -> Option<String> {
    YOUTUBE_PLAYLIST_CONTINUATIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|state| state.get(playlist_id).cloned())
        .filter(|value| !value.is_empty())
}

#[cfg(windows)]
fn decode_javascript_string(value: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        index += 1;
        let escaped = *bytes.get(index)?;
        match escaped {
            b'x' => {
                let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'n' => {
                decoded.push(b'\n');
                index += 1;
            }
            b'r' => {
                decoded.push(b'\r');
                index += 1;
            }
            b't' => {
                decoded.push(b'\t');
                index += 1;
            }
            other => {
                decoded.push(other);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(windows)]
fn playlist_tracks_from_payload(payload: &serde_json::Value) -> Vec<serde_json::Value> {
    fn find_string(value: &serde_json::Value, key: &str) -> Option<String> {
        value
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .or_else(|| {
                value
                    .as_array()
                    .and_then(|items| items.iter().find_map(|item| find_string(item, key)))
                    .or_else(|| {
                        value.as_object().and_then(|object| {
                            object.values().find_map(|item| find_string(item, key))
                        })
                    })
            })
    }
    fn visit(value: &serde_json::Value, tracks: &mut Vec<serde_json::Value>) {
        if let Some(row) = value.get("musicResponsiveListItemRenderer") {
            let video_id = row
                .pointer("/playlistItemData/videoId")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
                .or_else(|| find_string(row, "videoId"));
            let title = row
                .pointer(
                    "/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text",
                )
                .and_then(|value| value.as_str());
            if let (Some(video_id), Some(title)) = (video_id, title) {
                if !tracks.iter().any(|track| {
                    track.get("youtubeId").and_then(|value| value.as_str()) == Some(&video_id)
                }) {
                    let artist = row.pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text").and_then(|value| value.as_str()).unwrap_or("YouTube");
                    let duration = row.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text").and_then(|value| value.as_str()).unwrap_or("--:--");
                    let cover = row
                        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
                        .and_then(|value| value.as_array())
                        .and_then(|items| items.last())
                        .and_then(|value| value.get("url"))
                        .and_then(|value| value.as_str());
                    tracks.push(serde_json::json!({ "id": format!("youtube-{video_id}"), "youtubeId": video_id, "title": title, "artist": artist, "album": "YouTube Music", "duration": duration, "coverUrl": cover, "source": "youtube", "accent": "#ff5f55" }));
                }
            }
        }
        if let Some(items) = value.as_array() {
            for item in items {
                visit(item, tracks);
            }
        }
        if let Some(object) = value.as_object() {
            for item in object.values() {
                visit(item, tracks);
            }
        }
    }
    let mut tracks = Vec::new();
    visit(payload, &mut tracks);
    tracks
}

#[cfg(windows)]
fn playlist_continuation_from_payload(payload: &serde_json::Value) -> Option<String> {
    if let Some(token) = payload
        .pointer("/continuationItemRenderer/continuationEndpoint/continuationCommand/token")
        .and_then(|value| value.as_str())
    {
        return Some(token.to_string());
    }
    payload
        .as_array()
        .and_then(|items| items.iter().find_map(playlist_continuation_from_payload))
        .or_else(|| {
            payload
                .as_object()
                .and_then(|object| object.values().find_map(playlist_continuation_from_payload))
        })
}

#[cfg(windows)]
fn playlist_continuation_for(payload: &serde_json::Value, playlist_id: &str) -> Option<String> {
    let matches = payload.get("playlistId").and_then(|value| value.as_str()) == Some(playlist_id)
        || payload.get("targetId").and_then(|value| value.as_str()) == Some(playlist_id);
    if matches {
        if let Some(token) = playlist_continuation_from_payload(payload) {
            return Some(token);
        }
    }
    payload
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find_map(|item| playlist_continuation_for(item, playlist_id))
        })
        .or_else(|| {
            payload.as_object().and_then(|object| {
                object
                    .values()
                    .find_map(|item| playlist_continuation_for(item, playlist_id))
            })
        })
}

#[cfg(windows)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppWebviewInfo {
    label: String,
    name: String,
    visible: bool,
    internal: bool,
}

#[cfg(windows)]
fn webview_name(label: &str) -> (&'static str, bool) {
    match label {
        "main" => ("VibeDeck principal", false),
        "youtube-auth" => ("Login do YouTube Music", false),
        "youtube-stream-bridge" => ("Broker de streams de audio", true),
        _ => ("WebView auxiliar", true),
    }
}

#[cfg(windows)]
fn mute_webview(window: &tauri::WebviewWindow) -> Result<(), String> {
    let error = std::sync::Arc::new(Mutex::new(None::<String>));
    let callback_error = error.clone();
    window
        .with_webview(move |webview| {
            let result = unsafe { webview.controller().CoreWebView2() }
                .and_then(|core| {
                    core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8>()
                })
                .and_then(|core| unsafe { core.SetIsMuted(true) });
            if let Err(error) = result {
                if let Ok(mut stored) = callback_error.lock() {
                    *stored = Some(error.to_string());
                }
            }
        })
        .map_err(|error| error.to_string())?;
    let result = error
        .lock()
        .map_err(|error| error.to_string())?
        .take()
        .map_or(Ok(()), Err);
    result
}

#[cfg(windows)]
fn classify_log(
    message: &str,
) -> (
    &'static str,
    Option<&'static str>,
    &'static str,
    &'static str,
) {
    let lower = message.to_lowercase();
    let level = if lower.contains("failed")
        || lower.contains("error")
        || lower.contains("invalid")
        || lower.contains("timed out")
        || lower.contains("tempo limite")
    {
        "error"
    } else {
        "info"
    };
    if lower.contains("login") || lower.contains("youtube-auth") {
        return (level, Some("youtube-auth"), "auth", "youtube-auth");
    }
    if lower.contains("library") {
        return (level, None, "library", "youtube-library");
    }
    if lower.contains("playlist") {
        return (level, None, "playlist", "youtube-playlist");
    }
    if lower.contains("stream bridge")
        || lower.contains("googlevideo")
        || lower.contains("audio stream")
    {
        return (
            level,
            Some("youtube-stream-bridge"),
            "stream",
            "youtube-stream",
        );
    }
    if lower.contains("audio") || lower.contains("player") {
        return (level, None, "audio", "audio-engine");
    }
    if lower.contains("search") {
        return (level, None, "search", "youtube-search");
    }
    if lower.contains("session") || lower.contains("cookie") {
        return (level, None, "session", "youtube-session");
    }
    (level, None, "app", "native")
}

#[cfg(windows)]
#[tauri::command]
fn start_oauth_listener() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let callback = OAUTH_CALLBACK.get_or_init(|| Mutex::new(None));
    *callback.lock().map_err(|error| error.to_string())? = None;
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0_u8; 8192];
            if let Ok(size) = stream.read(&mut buffer) {
                let request = String::from_utf8_lossy(&buffer[..size]);
                if let Some(path) = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                {
                    if let Some(storage) = OAUTH_CALLBACK.get() {
                        if let Ok(mut value) = storage.lock() {
                            *value = Some(path.to_string());
                        }
                    }
                }
            }
            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>VibeDeck conectado</title><style>*{box-sizing:border-box}body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0b0d12;color:#f4f5f7;font-family:Arial,sans-serif}.card{width:min(520px,calc(100% - 32px));padding:42px;border:1px solid #29313b;border-radius:22px;background:linear-gradient(145deg,#151b20,#101319);text-align:center;box-shadow:0 24px 80px #0008}.mark{width:58px;height:58px;margin:0 auto 22px;display:grid;place-items:center;border-radius:17px;color:#0b0d12;background:#b9f36b;font-size:30px;font-weight:700}.eyebrow{margin:0 0 12px;color:#9ca8a1;font-size:11px;font-weight:700;letter-spacing:2px}.title{margin:0;font-size:28px}.copy{margin:14px auto 28px;max-width:360px;color:#929ba1;line-height:1.6}.countdown{color:#b9f36b;font-size:13px}.countdown strong{font-size:18px}.hint{margin-top:22px;color:#68737a;font-size:11px}@media(max-width:480px){.card{padding:30px 22px}.title{font-size:23px}}</style></head><body><main class=\"card\"><div class=\"mark\">V</div><p class=\"eyebrow\">VIBEDECK</p><h1 class=\"title\">Conta conectada</h1><p class=\"copy\">A autorizaÃ§Ã£o do Google foi concluÃ­da. VocÃª jÃ¡ pode voltar ao aplicativo e sincronizar suas bibliotecas.</p><div class=\"countdown\">Esta janela tentarÃ¡ fechar em <strong id=\"seconds\">5</strong>s.</div><p class=\"hint\">Se ela nÃ£o fechar automaticamente, pode fechÃ¡-la com seguranÃ§a.</p></main><script>let seconds=5;const label=document.getElementById('seconds');const timer=setInterval(()=>{seconds-=1;label.textContent=seconds;if(seconds<=0){clearInterval(timer);window.close()}},1000)</script></body></html>";
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Ok(port)
}

#[cfg(windows)]
fn write_native_log(app: &tauri::AppHandle, message: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let (level, window, operation, source) = classify_log(message);
    let entry = AppLogEntry {
        id: APP_LOG_ID.fetch_add(1, Ordering::Relaxed),
        timestamp,
        level,
        source,
        window,
        operation: operation.to_string(),
        message: message.to_string(),
        code: None,
        correlation_id: None,
        details: None,
    };
    if let Ok(mut logs) = APP_LOGS.get_or_init(|| Mutex::new(Vec::new())).lock() {
        logs.push(entry.clone());
        if logs.len() > 2000 {
            let overflow = logs.len() - 2000;
            logs.drain(0..overflow);
        }
    }
    let _ = app.emit("app-log", entry);
    if let Ok(directory) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("vibedeck.log"))
            .and_then(|mut file| writeln!(file, "[{timestamp}] {message}"));
    }
}

#[cfg(windows)]
#[tauri::command]
fn get_app_logs() -> Vec<AppLogEntry> {
    APP_LOGS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map(|logs| logs.clone())
        .unwrap_or_default()
}

#[cfg(windows)]
#[tauri::command]
fn get_youtube_playlist_debug_state() -> PlaylistBridgeDebugState {
    YOUTUBE_PLAYLIST_DEBUG
        .get_or_init(|| Mutex::new(PlaylistBridgeDebugState::default()))
        .lock()
        .map(|state| state.clone())
        .unwrap_or_default()
}

#[cfg(windows)]
#[tauri::command]
fn clear_app_logs() {
    if let Ok(mut logs) = APP_LOGS.get_or_init(|| Mutex::new(Vec::new())).lock() {
        logs.clear();
    }
}

#[cfg(windows)]
#[tauri::command]
fn write_app_log(app: tauri::AppHandle, request: AppLogRequest) {
    let message = format!("UI {}", request.message);
    let (level, window, _operation, source) = classify_log(&message);
    let entry = AppLogEntry {
        id: APP_LOG_ID.fetch_add(1, Ordering::Relaxed),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default(),
        level: if request.level.as_deref() == Some("error") {
            "error"
        } else {
            level
        },
        source,
        window,
        operation: request.operation,
        message,
        code: request.code,
        correlation_id: Some(request.correlation_id),
        details: request.details,
    };
    if let Ok(mut logs) = APP_LOGS.get_or_init(|| Mutex::new(Vec::new())).lock() {
        logs.push(entry.clone());
        if logs.len() > 2000 {
            let overflow = logs.len() - 2000;
            logs.drain(0..overflow);
        }
    }
    let _ = app.emit("app-log", entry.clone());
    if let Ok(directory) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(&directory);
        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("vibedeck.log"))
                .and_then(|mut file| writeln!(file, "{line}"));
        }
    }
}

#[cfg(windows)]
#[tauri::command]
fn get_app_webviews(app: tauri::AppHandle) -> Vec<AppWebviewInfo> {
    app.webview_windows()
        .into_iter()
        .map(|(label, window)| {
            let label = label.to_string();
            let (name, internal) = webview_name(&label);
            AppWebviewInfo {
                label,
                name: name.to_string(),
                visible: window.is_visible().unwrap_or(false),
                internal,
            }
        })
        .collect()
}

#[cfg(windows)]
#[tauri::command]
fn set_app_webview_visible(
    app: tauri::AppHandle,
    label: String,
    visible: bool,
) -> Result<(), String> {
    if label == "main" {
        return Err(
            "A janela principal nÃ£o pode ser ocultada pelo painel de diagnÃ³stico.".to_string(),
        );
    }
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("A WebView '{label}' nÃ£o estÃ¡ ativa."))?;
    if visible {
        window.show()
    } else {
        window.hide()
    }
    .map_err(|error| error.to_string())?;
    write_native_log(
        &app,
        &format!("WebView visibility changed: label={label}, visible={visible}"),
    );
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
fn export_app_logs(app: tauri::AppHandle) -> Result<String, String> {
    let logs = APP_LOGS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|error| error.to_string())?
        .clone();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(format!("vibedeck-logs-{timestamp}.log"));
    let content = logs
        .into_iter()
        .map(|entry| {
            format!(
                "[{}] [{}] [{}] [{}] {}",
                entry.timestamp,
                entry.level.to_uppercase(),
                entry.source,
                entry.operation,
                entry.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, content).map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(windows)]
#[tauri::command]
fn take_oauth_callback() -> Option<String> {
    OAUTH_CALLBACK.get()?.lock().ok()?.take()
}

#[cfg(windows)]
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let escaped_url = url.replace('\'', "''");
    let command = format!("Start-Process -FilePath '{}'", escaped_url);
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &command])
        .spawn()
        .map_err(|error| error.to_string())
        .map(|_| ())
}

#[cfg(windows)]
#[tauri::command]
fn save_secure_record(app: tauri::AppHandle, record: String) -> Result<(), String> {
    use tauri::Manager;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("secure-record.bin");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, record).map_err(|error| error.to_string())
}

#[cfg(windows)]
#[tauri::command]
async fn open_youtube_login(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    let _login_lock = YOUTUBE_LOGIN_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    if let Some(window) = app.get_webview_window("youtube-auth") {
        let _ = window.set_focus();
        write_native_log(&app, "Reusing existing YouTube login webview");
        return Ok(());
    }
    write_native_log(&app, "Creating YouTube login webview");
    let url = "https://accounts.google.com/v3/signin/identifier?continue=https://www.youtube.com/signin?action_handle_signin%3Dtrue%26app%3Ddesktop%26hl%3Dpt%26next%3Dhttps%253A%252F%252Fmusic.youtube.com%252F%26feature%3D__FEATURE__&hl=pt-BR&ltmpl=music&passive=true&service=youtube&uilel=3&flowName=GlifWebSignIn&flowEntry=ServiceLogin".parse().map_err(|error: url::ParseError| error.to_string())?;
    let window =
        match WebviewWindowBuilder::new(&app, "youtube-auth", WebviewUrl::App("index.html".into()))
            .title("VibeDeck - Entrar no YouTube Music")
            .inner_size(1100.0, 760.0)
            .resizable(true)
            .on_page_load(|window, payload| {
                write_native_log(
                    window.app_handle(),
                    &format!("YouTube login page load: {:?}", payload.event()),
                );
            })
            .build()
        {
            Ok(window) => window,
            Err(_) => {
                write_native_log(&app, "YouTube login build failed");
                return Err("youtube_login_build_failed".into());
            }
        };
    write_native_log(&app, "Navigating YouTube login webview");
    window
        .navigate(url)
        .map_err(|_| "youtube_login_navigation_failed")?;
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
async fn capture_youtube_session(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri::Manager;
    let window = app
        .get_webview_window("youtube-auth")
        .ok_or_else(|| "A janela de login do YouTube Music nÃ£o estÃ¡ aberta.".to_string())?;
    let session = youtube_cookie_bridge::capture(&window).await?;
    if !session.has_auth_cookie(unix_now()?) {
        return Ok(false);
    }
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("youtube-session.bin");
    session.save(&path)?;
    if let Some(window) = app.get_webview_window("youtube-auth") {
        let _ = window.close();
    }
    Ok(true)
}

#[cfg(windows)]
#[tauri::command]
fn youtube_session_available(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri::Manager;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("youtube-session.bin");
    let now = unix_now()?;
    // Availability is local validity, never a claim of server acceptance.
    Ok(YouTubeSession::load(&path).is_ok_and(|session| session.has_auth_cookie(now)))
}

#[cfg(windows)]
#[tauri::command]
async fn logout_youtube_music(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("youtube-auth") {
        tauri::async_runtime::spawn_blocking(move || window.clear_all_browsing_data())
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        if let Some(window) = app.get_webview_window("youtube-auth") {
            window.close().map_err(|error| error.to_string())?;
        }
    }
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    for name in ["youtube-session.bin", "secure-record.bin"] {
        let path = data_dir.join(name);
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    write_native_log(&app, "YouTube session and stored credentials cleared");
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
fn load_secure_record(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri::Manager;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("secure-record.bin");
    match std::fs::read_to_string(path) {
        Ok(record) => Ok(Some(record)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(windows)]
#[tauri::command]
async fn exchange_google_code(
    code: String,
    client_id: String,
    redirect_uri: String,
    verifier: String,
) -> Result<String, String> {
    let client_secret = option_env!("VIBE_GOOGLE_CLIENT_SECRET")
        .ok_or_else(|| "GOOGLE_CLIENT_SECRET nÃ£o foi configurado antes do build.".to_string())?;
    let response = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", client_id),
            ("client_secret", client_secret.to_string()),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code".to_string()),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(body);
    }
    Ok(body)
}

#[cfg(windows)]
#[tauri::command]
async fn refresh_google_token(refresh_token: String, client_id: String) -> Result<String, String> {
    let mut form = vec![
        ("refresh_token", refresh_token),
        ("client_id", client_id),
        ("grant_type", "refresh_token".to_string()),
    ];
    if let Some(client_secret) = option_env!("VIBE_GOOGLE_CLIENT_SECRET") {
        form.push(("client_secret", client_secret.to_string()));
    }
    let response = reqwest::Client::new()
        .post("https://oauth2.googleapis.com/token")
        .form(&form)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(body);
    }
    Ok(body)
}

/// Result of one authenticated `youtubei/v1/player` request. `cookie_header`
/// is a live credential and must stay in memory.
#[cfg(windows)]
struct PlayerFetch {
    payload: serde_json::Value,
    client_version: String,
    visitor_id: String,
    cookie_header: String,
}

/// Performs the authenticated `youtubei/v1/player` request and returns the raw
/// payload plus the InnerTube session context required by follow-up requests.
#[cfg(windows)]
async fn fetch_youtube_player(
    app: &tauri::AppHandle,
    video_id: &str,
) -> Result<PlayerFetch, String> {
    fetch_youtube_player_with_sts(app, video_id, None).await
}

/// Same request; when `signature_timestamp` is set it is sent in `playbackContext`, so the
/// server scrambles `s` for the player revision that declares that timestamp.
#[cfg(windows)]
async fn fetch_youtube_player_with_sts(
    app: &tauri::AppHandle,
    video_id: &str,
    signature_timestamp: Option<u32>,
) -> Result<PlayerFetch, String> {
    use tauri::Manager;
    let session_path = app
        .path()
        .app_data_dir()
        .map_err(|_| "youtube_session_directory_failed")?
        .join("youtube-session.bin");
    let session = YouTubeSession::load(&session_path)?;
    if !session.has_auth_cookie(unix_now()?) {
        return Err("youtube_session_missing_auth_cookie".into());
    }
    let client = youtube_http::client()?;
    let visitor_response = client
        .get("https://music.youtube.com")
        .header(
            "Cookie",
            session.cookie_header_for("https://music.youtube.com/", unix_now()?)?,
        )
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .send()
        .await
        .map_err(|error| youtube_http::request_error(&error))?;
    let visitor_bytes = youtube_http::bounded_body(visitor_response, 4 * 1024 * 1024).await?;
    let visitor_html =
        std::str::from_utf8(&visitor_bytes).map_err(|_| "youtube_config_invalid_encoding")?;
    let read_config = |name: &str| {
        regex::Regex::new(&format!(r#"{name}":"([^\"]+)""#))
            .ok()
            .and_then(|pattern| pattern.captures(&visitor_html))
            .and_then(|capture| capture.get(1))
            .map(|value| value.as_str().to_string())
            .unwrap_or_default()
    };
    let visitor_id = read_config("VISITOR_DATA");
    let client_version = read_config("INNERTUBE_CLIENT_VERSION");
    let api_key = read_config("INNERTUBE_API_KEY");
    let page_gl = read_config("GL");
    let page_hl = read_config("HL");
    let gl = if page_gl.is_empty() {
        "US".to_string()
    } else {
        page_gl
    };
    let hl = if page_hl.is_empty() {
        "en".to_string()
    } else {
        page_hl
    };
    if visitor_id.is_empty() || client_version.is_empty() {
        return Err("youtube_config_missing_context".into());
    }
    write_native_log(&app, "InnerTube config: context_present=true");
    let mut body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": client_version,
                "hl": hl,
                "gl": gl,
                "visitorData": visitor_id,
                "platform": "DESKTOP",
                "clientFormFactor": "UNKNOWN_FORM_FACTOR"
            },
            "user": { "lockedSafetyMode": false },
            "request": { "useSsl": true }
        },
        "videoId": video_id,
        "contentCheckOk": true,
        "racyCheckOk": true
    });
    if let Some(timestamp) = signature_timestamp {
        body["playbackContext"] = serde_json::json!({
            "contentPlaybackContext": { "signatureTimestamp": timestamp }
        });
    }
    let endpoint = if api_key.is_empty() {
        "https://music.youtube.com/youtubei/v1/player?alt=json".to_string()
    } else {
        format!("https://music.youtube.com/youtubei/v1/player?alt=json&key={api_key}")
    };
    let timestamp = unix_now()?;
    let cookie_header = session.cookie_header_for(&endpoint, timestamp)?;
    let authorization = session.authorization(&endpoint, timestamp as u64, None)?;
    let response = client
        .post(endpoint)
        .header("Authorization", authorization)
        .header("Cookie", &cookie_header)
        .header("X-Youtube-Client-Name", "67")
        .header("X-Youtube-Client-Version", &client_version)
        .header("X-Origin", "https://music.youtube.com")
        .header("X-Goog-AuthUser", "0")
        .header("X-Youtube-Bootstrap-Logged-In", "true")
        .header(
            "X-Goog-Request-Time",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_secs())
                .unwrap_or_default()
                .to_string(),
        )
        .header("Origin", "https://music.youtube.com")
        .header("X-Goog-Visitor-Id", &visitor_id)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .json(&body)
        .send()
        .await
        .map_err(|error| youtube_http::request_error(&error))?;
    let bytes = youtube_http::bounded_body(response, 4 * 1024 * 1024).await?;
    let payload = youtube_http::player_json(&bytes)?;
    write_native_log(
        &app,
        &format!(
            "Player status: {}",
            youtube_http::playability_status(&payload)
        ),
    );
    Ok(PlayerFetch {
        payload,
        client_version,
        visitor_id,
        cookie_header,
    })
}

#[cfg(windows)]
#[tauri::command]
async fn resolve_youtube_audio(
    app: tauri::AppHandle,
    video_id: String,
) -> Result<serde_json::Value, String> {
    write_native_log(&app, "Resolving online audio");
    let PlayerFetch { payload, .. } = fetch_youtube_player(&app, &video_id).await?;
    let formats = payload
        .get("streamingData")
        .and_then(|data| data.get("adaptiveFormats"))
        .and_then(|formats| formats.as_array())
        .ok_or_else(|| "O YouTube nÃ£o retornou formatos de Ã¡udio para esta faixa.".to_string())?;
    let selected = formats
        .iter()
        .filter(|format| {
            format
                .get("mimeType")
                .and_then(|value| value.as_str())
                .is_some_and(|mime| mime.starts_with("audio/") && format.get("url").is_some())
        })
        .max_by_key(|format| {
            format
                .get("bitrate")
                .and_then(|value| value.as_u64())
                .unwrap_or(0)
        })
        .ok_or_else(|| {
            write_native_log(&app, "No direct audio format returned");
            "Nenhum stream de Ã¡udio direto foi disponibilizado.".to_string()
        })?;
    write_native_log(&app, "Online audio stream resolved successfully");
    Ok(
        serde_json::json!({ "url": selected.get("url"), "mimeType": selected.get("mimeType"), "bitrate": selected.get("bitrate"), "expiresInSeconds": payload.get("streamingData").and_then(|data| data.get("expiresInSeconds")), "durationSeconds": payload.get("videoDetails").and_then(|details| details.get("lengthSeconds")) }),
    )
}

/// Maps YouTube's playability reason text to a fixed category so diagnostics
/// never carry provider prose.
#[cfg(all(windows, debug_assertions))]
fn categorize_playability_reason(reason: &str) -> &'static str {
    let reason = reason.to_ascii_lowercase();
    if reason.is_empty() {
        "none"
    } else if reason.contains("not a bot") || reason.contains("bot") {
        "bot_check"
    } else if reason.contains("sign in") || reason.contains("login") {
        "login_required"
    } else if reason.contains("age") {
        "age_restricted"
    } else if reason.contains("country") || reason.contains("location") {
        "geo_restricted"
    } else if reason.contains("private") {
        "private"
    } else if reason.contains("removed") || reason.contains("deleted") {
        "removed"
    } else if reason.contains("not available") || reason.contains("unavailable") {
        "unavailable"
    } else {
        "other"
    }
}

/// Debug-only Gate A probe for the native SABR client. It issues the same
/// authenticated player request the app already makes and reports only
/// booleans, counts, and fixed status codes, so no media URL, token, cookie,
/// or track identifier leaves the process.
#[cfg(all(windows, debug_assertions))]
#[tauri::command]
async fn debug_youtube_sabr_probe(
    app: tauri::AppHandle,
    video_id: String,
) -> Result<serde_json::Value, String> {
    let fetch = fetch_youtube_player(&app, &video_id).await?;
    let payload = &fetch.payload;
    let streaming = payload.get("streamingData");
    let audio: Vec<&serde_json::Value> = streaming
        .and_then(|data| data.get("adaptiveFormats"))
        .and_then(|formats| formats.as_array())
        .map(|formats| {
            formats
                .iter()
                .filter(|format| {
                    format
                        .get("mimeType")
                        .and_then(|value| value.as_str())
                        .is_some_and(|mime| mime.starts_with("audio/"))
                })
                .collect()
        })
        .unwrap_or_default();
    let count_with = |name: &str| {
        audio
            .iter()
            .filter(|format| format.get(name).is_some())
            .count()
    };
    let streaming_url = streaming
        .and_then(|data| data.get("serverAbrStreamingUrl"))
        .and_then(|value| value.as_str());
    let host_is_googlevideo = streaming_url
        .and_then(|url| url::Url::parse(url).ok())
        .and_then(|url| url.host_str().map(|host| host.to_string()))
        .is_some_and(|host| host == "googlevideo.com" || host.ends_with(".googlevideo.com"));
    let ustreamer_bytes = sabr::player_ustreamer_config(payload)
        .map(|bytes| bytes.len())
        .unwrap_or_default();
    let po_token_bytes = payload
        .pointer("/serviceIntegrityDimensions/poToken")
        .or_else(|| payload.pointer("/streamingData/serviceIntegrityDimensions/poToken"))
        .and_then(|value| value.as_str())
        .and_then(sabr::proto::decode_base64)
        .map(|bytes| bytes.len())
        .unwrap_or_default();
    let bootstrap =
        sabr::SabrBootstrap::from_player_payload(payload, &video_id, &fetch.client_version);
    let playability = youtube_http::playability_status(payload);
    let reason_category = categorize_playability_reason(
        payload
            .pointer("/playabilityStatus/reason")
            .and_then(|value| value.as_str())
            .unwrap_or_default(),
    );
    let has_error_screen = payload.pointer("/playabilityStatus/errorScreen").is_some();
    write_native_log(
        &app,
        &format!(
            "SABR Gate A probe: playability={playability}, reason={reason_category}, error_screen={has_error_screen}, server_abr={}, ustreamer_bytes={ustreamer_bytes}, audio_formats={}",
            streaming_url.is_some(),
            audio.len()
        ),
    );
    Ok(serde_json::json!({
        "playabilityStatus": playability,
        "playabilityReasonCategory": reason_category,
        "hasErrorScreen": has_error_screen,
        "hasServerAbrUrl": streaming_url.is_some(),
        "serverAbrHostIsGooglevideo": host_is_googlevideo,
        "hasUstreamerConfig": ustreamer_bytes > 0,
        "ustreamerConfigBytes": ustreamer_bytes,
        "audioFormatCount": audio.len(),
        "withLastModified": count_with("lastModified"),
        "withXtags": count_with("xtags"),
        "withDirectUrl": count_with("url"),
        "withSignatureCipher": count_with("signatureCipher"),
        "poTokenBytes": po_token_bytes,
        "durationPresent": payload.pointer("/videoDetails/lengthSeconds").is_some(),
        "bootstrap": bootstrap
            .as_ref()
            .map(|_| "ok")
            .unwrap_or_else(|error| error.code()),
    }))
}

#[cfg(windows)]
#[tauri::command]
async fn search_youtube_music(
    app: tauri::AppHandle,
    query: String,
) -> Result<Vec<serde_json::Value>, String> {
    use tauri::Manager;
    write_native_log(&app, "YouTube search started");
    let session_path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("youtube-session.bin");
    let session = YouTubeSession::load(&session_path)?;
    let client = youtube_http::client()?;
    let visitor_response = client
        .get("https://music.youtube.com")
        .header(
            "Cookie",
            session.cookie_header_for("https://music.youtube.com/", unix_now()?)?,
        )
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153 Safari/537.36",
        )
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let visitor_html = visitor_response
        .text()
        .await
        .map_err(|error| error.to_string())?;
    let read_config = |name: &str| {
        regex::Regex::new(&format!(r#"{name}":"([^\"]+)"#))
            .ok()
            .and_then(|pattern| pattern.captures(&visitor_html))
            .and_then(|capture| capture.get(1))
            .map(|value| value.as_str().to_string())
            .unwrap_or_default()
    };
    let visitor_id = read_config("VISITOR_DATA");
    let client_version = read_config("INNERTUBE_CLIENT_VERSION");
    let body = serde_json::json!({
        "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": client_version, "hl": "pt", "gl": "BR", "visitorData": visitor_id, "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153 Safari/537.36", "osName": "Windows", "osVersion": "10.0", "originalUrl": "https://music.youtube.com/", "platform": "DESKTOP", "clientFormFactor": "UNKNOWN_FORM_FACTOR", "browserName": "Chrome", "browserVersion": "153" }, "user": { "lockedSafetyMode": false }, "request": { "useSsl": true, "internalExperimentFlags": [], "consistencyTokenJars": [] } },
        "query": query,
        "suggestStats": { "validationStatus": "VALID", "parameterValidationStatus": "VALID_PARAMETERS", "clientName": "youtube-music", "searchMethod": "ENTER_KEY", "inputMethods": ["KEYBOARD"], "originalQuery": query, "availableSuggestions": [], "zeroPrefixEnabled": true },
        "inlineSettingStatus": "INLINE_SETTING_STATUS_ON"
    });
    let endpoint = "https://music.youtube.com/youtubei/v1/search?prettyPrint=false";
    let timestamp = unix_now()?;
    let cookie_header = session.cookie_header_for(endpoint, timestamp)?;
    let authorization = session.authorization(endpoint, timestamp as u64, None)?;
    let response = client
        .post(endpoint)
        .header("Authorization", authorization)
        .header("Cookie", cookie_header)
        .header("X-Youtube-Client-Name", "67")
        .header("X-Youtube-Client-Version", &client_version)
        .header("X-Origin", "https://music.youtube.com")
        .header("X-Goog-AuthUser", "0")
        .header("X-Goog-Visitor-Id", visitor_id)
        .header("X-Youtube-Bootstrap-Logged-In", "true")
        .header("Referer", "https://music.youtube.com/")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153 Safari/537.36",
        )
        .json(&body)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let payload = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| error.to_string())?;
    if !status.is_success() {
        write_native_log(&app, &format!("YouTube search failed: HTTP {status}"));
        return Err(format!("YouTube search HTTP {status}"));
    }
    let mut results = Vec::new();
    collect_music_results(&payload, &mut results);
    results.truncate(25);
    write_native_log(
        &app,
        &format!("YouTube search returned {} results", results.len()),
    );
    Ok(results)
}

#[cfg(windows)]
#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
struct YouTubeHealthCheckItem {
    operation: &'static str,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
}

#[cfg(windows)]
#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct YouTubeHealthCheckReport {
    checks: Vec<YouTubeHealthCheckItem>,
    player: YouTubeHealthCheckPlayer,
    audio: serde_json::Value,
    ump_capture: UmpCaptureSnapshot,
}

#[cfg(windows)]
#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
struct UmpPartKindCount {
    kind: u32,
    count: u32,
}

#[cfg(windows)]
#[derive(Clone, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
struct UmpCaptureSnapshot {
    scope: &'static str,
    stage: &'static str,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    activation_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    same_stream_request: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    used_capture_as_playback_url: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_context: Option<audio::AudioStreamRequestContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_headers: Option<audio::AudioRequestHeaderSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_body: Option<audio::AudioRequestBodySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rust_parser_outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rust_extracted_media_bytes: Option<u64>,
    body_bytes: u64,
    media_bytes: u64,
    part_kinds: Vec<UmpPartKindCount>,
}

#[cfg(windows)]
static UMP_CAPTURE_DIAGNOSTICS: OnceLock<Mutex<UmpCaptureSnapshot>> = OnceLock::new();

#[cfg(windows)]
fn ump_capture_snapshot() -> UmpCaptureSnapshot {
    UMP_CAPTURE_DIAGNOSTICS
        .get_or_init(|| {
            Mutex::new(UmpCaptureSnapshot {
                scope: "webview_capture_only_not_audio_engine_input_proof",
                stage: "waiting",
                outcome: "not_attempted",
                activation_sequence: None,
                http_status: None,
                content_type: None,
                same_stream_request: None,
                used_capture_as_playback_url: None,
                request_context: None,
                request_headers: None,
                request_body: None,
                rust_parser_outcome: None,
                rust_extracted_media_bytes: None,
                body_bytes: 0,
                media_bytes: 0,
                part_kinds: Vec::new(),
            })
        })
        .lock()
        .map(|snapshot| snapshot.clone())
        .unwrap_or(UmpCaptureSnapshot {
            scope: "webview_capture_only_not_audio_engine_input_proof",
            stage: "waiting",
            outcome: "unavailable",
            activation_sequence: None,
            http_status: None,
            content_type: None,
            same_stream_request: None,
            used_capture_as_playback_url: None,
            request_context: None,
            request_headers: None,
            request_body: None,
            rust_parser_outcome: None,
            rust_extracted_media_bytes: None,
            body_bytes: 0,
            media_bytes: 0,
            part_kinds: Vec::new(),
        })
}

#[cfg(windows)]
fn update_ump_capture(snapshot: UmpCaptureSnapshot) {
    if let Ok(mut current) = UMP_CAPTURE_DIAGNOSTICS
        .get_or_init(|| Mutex::new(snapshot.clone()))
        .lock()
    {
        *current = snapshot;
    }
}

#[cfg(windows)]
fn ump_capture_progress(activation_sequence: u64) {
    update_ump_capture(UmpCaptureSnapshot {
        scope: "webview_capture_only_not_audio_engine_input_proof",
        stage: "waiting",
        outcome: "in_progress",
        activation_sequence: Some(activation_sequence),
        http_status: None,
        content_type: None,
        same_stream_request: None,
        used_capture_as_playback_url: None,
        request_context: None,
        request_headers: None,
        request_body: None,
        rust_parser_outcome: None,
        rust_extracted_media_bytes: None,
        body_bytes: 0,
        media_bytes: 0,
        part_kinds: Vec::new(),
    });
}

#[cfg(windows)]
fn ump_capture_response_observed(
    snapshot: &mut UmpCaptureSnapshot,
    status: i32,
    content_type: &str,
    request_uri: &str,
    request_headers: Option<audio::AudioRequestHeaderSummary>,
    request_body: Option<audio::AudioRequestBodySummary>,
) {
    snapshot.stage = "capture";
    snapshot.outcome = "in_progress";
    snapshot.http_status = u16::try_from(status).ok();
    snapshot.content_type = Some(
        if content_type
            .split(';')
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/vnd.yt-ump"))
        {
            "application/vnd.yt-ump"
        } else {
            "other"
        },
    );
    snapshot.same_stream_request = None;
    snapshot.request_context = url::Url::parse(request_uri)
        .ok()
        .map(|parsed| audio::audio_request_context(&parsed, "POST"));
    snapshot.request_headers = request_headers;
    snapshot.request_body = request_body;
}

#[cfg(windows)]
fn ump_capture_mark_failure(snapshot: &mut UmpCaptureSnapshot, outcome: &'static str) {
    snapshot.stage = "capture";
    snapshot.outcome = outcome;
    snapshot.body_bytes = 0;
    snapshot.media_bytes = 0;
    snapshot.part_kinds.clear();
}

#[cfg(windows)]
fn ump_capture_failure(outcome: &'static str) {
    let mut snapshot = ump_capture_snapshot();
    ump_capture_mark_failure(&mut snapshot, outcome);
    update_ump_capture(snapshot);
}

#[cfg(windows)]
fn ump_capture_record_playback_url_selection(used_capture_url: bool) {
    if let Some(state) = UMP_CAPTURE_DIAGNOSTICS.get() {
        if let Ok(mut snapshot) = state.lock() {
            snapshot.used_capture_as_playback_url = Some(used_capture_url);
        }
    }
}

#[cfg(windows)]
struct UmpCaptureAttemptGuard {
    completed: bool,
}

#[cfg(windows)]
impl Drop for UmpCaptureAttemptGuard {
    fn drop(&mut self) {
        if !self.completed {
            ump_capture_failure("capture_failed");
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Serialize, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct YouTubeHealthCheckPlayer {
    status: &'static str,
    source: &'static str,
    position_seconds: Option<f64>,
    duration_seconds: Option<f64>,
    queue_length: usize,
    queue_index: i64,
    queue_generation: u64,
    activation_sequence: u64,
    native_mode: bool,
}

#[cfg(windows)]
fn health_check_player_status(has_activation: bool, active: bool, paused: bool) -> &'static str {
    if !has_activation {
        "idle"
    } else if !active {
        "stopped"
    } else if paused {
        "paused"
    } else {
        "playing"
    }
}

#[cfg(windows)]
fn health_check_player_source(source: Option<&str>) -> &'static str {
    match source {
        Some("youtube") => "youtube",
        Some("local") => "local",
        Some(_) => "other",
        None => "none",
    }
}

#[cfg(windows)]
fn finite_nonnegative(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

#[cfg(windows)]
fn health_check_player_snapshot() -> YouTubeHealthCheckPlayer {
    let snapshot = audio::AudioEngine::snapshot();
    let active = audio::AudioEngine::is_active();
    let paused = audio::AudioEngine::is_paused();
    YouTubeHealthCheckPlayer {
        status: health_check_player_status(snapshot.native_mode, active, paused),
        source: health_check_player_source(snapshot.source.as_deref()),
        position_seconds: finite_nonnegative(snapshot.position),
        duration_seconds: finite_nonnegative(snapshot.duration),
        queue_length: snapshot.queue.len(),
        queue_index: snapshot.queue_index,
        queue_generation: snapshot.queue_generation,
        activation_sequence: snapshot.activation_sequence,
        native_mode: snapshot.native_mode,
    }
}

#[cfg(windows)]
fn health_check_error_code(error: &str) -> String {
    if let Some(status) = error
        .split("HTTP ")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
    {
        if status.len() == 3 && status.bytes().all(|byte| byte.is_ascii_digit()) {
            return format!("http_{status}");
        }
    }
    if error.contains("timed out") || error.contains("Tempo limite") {
        "timeout".to_string()
    } else if error.contains("session") || error.contains("sess") || error.contains("login") {
        "session_unavailable".to_string()
    } else {
        "request_failed".to_string()
    }
}

#[cfg(windows)]
fn skipped_health_check(operation: &'static str) -> YouTubeHealthCheckItem {
    YouTubeHealthCheckItem {
        operation,
        status: "skipped",
        count: None,
        error_code: None,
    }
}

#[cfg(windows)]
#[tauri::command]
async fn youtube_music_healthcheck(app: tauri::AppHandle) -> YouTubeHealthCheckReport {
    let session_available = youtube_session_available(app.clone()).unwrap_or(false);
    let mut checks = vec![YouTubeHealthCheckItem {
        operation: "session",
        status: if session_available {
            "passed"
        } else {
            "failed"
        },
        count: None,
        error_code: if session_available {
            None
        } else {
            Some("session_unavailable".to_string())
        },
    }];
    if !session_available {
        checks.push(skipped_health_check("search"));
        checks.push(skipped_health_check("playlists"));
        return YouTubeHealthCheckReport {
            checks,
            player: health_check_player_snapshot(),
            audio: serde_json::to_value(audio::AudioEngine::diagnostics_snapshot())
                .unwrap_or(serde_json::Value::Null),
            ump_capture: ump_capture_snapshot(),
        };
    }

    let search = search_youtube_music(app.clone(), "VibeDeck healthcheck".to_string()).await;
    checks.push(match search {
        Ok(results) => YouTubeHealthCheckItem {
            operation: "search",
            status: "passed",
            count: Some(results.len()),
            error_code: None,
        },
        Err(error) => YouTubeHealthCheckItem {
            operation: "search",
            status: "failed",
            count: None,
            error_code: Some(health_check_error_code(&error)),
        },
    });
    let playlists = load_youtube_playlists(app).await;
    checks.push(match playlists {
        Ok(items) => YouTubeHealthCheckItem {
            operation: "playlists",
            status: "passed",
            count: Some(items.len()),
            error_code: None,
        },
        Err(error) => YouTubeHealthCheckItem {
            operation: "playlists",
            status: "failed",
            count: None,
            error_code: Some(health_check_error_code(&error)),
        },
    });
    YouTubeHealthCheckReport {
        checks,
        player: health_check_player_snapshot(),
        audio: serde_json::to_value(audio::AudioEngine::diagnostics_snapshot())
            .unwrap_or(serde_json::Value::Null),
        ump_capture: ump_capture_snapshot(),
    }
}

#[cfg(windows)]
#[tauri::command]
async fn load_youtube_playlists(app: tauri::AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let _library_lock = YOUTUBE_LIBRARY_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    match youtube_browse::load_library_playlists(&app).await {
        Ok(playlists) if !playlists.is_empty() => {
            write_native_log(
                &app,
                &format!("YouTube library loaded natively: {} playlists", playlists.len()),
            );
            eprintln!("VibeDeck library path=native_browse playlists={}", playlists.len());
            Ok(playlists)
        }
        Ok(_) => {
            write_native_log(&app, "YouTube native library returned no playlists");
            Ok(Vec::new())
        }
        Err(code) => {
            write_native_log(&app, &format!("YouTube native library failed ({code})"));
            Err("Nao foi possivel carregar as playlists do YouTube Music. Sincronize a sessao novamente.".to_string())
        }
    }
}

#[cfg(all(windows, debug_assertions))]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct YouTubePlaylistProbeStatus {
    read_succeeded: bool,
    playlist_count: usize,
    result: &'static str,
}

/// Debug-only, read-only probe that exercises the authenticated library WebView
/// using the same native path as normal playlist synchronization. It deliberately
/// returns no playlist names, IDs, cookies, or provider payloads.
#[cfg(all(windows, debug_assertions))]
#[tauri::command]
async fn debug_youtube_playlist_probe_status(app: tauri::AppHandle) -> YouTubePlaylistProbeStatus {
    match load_youtube_playlists(app).await {
        Ok(playlists) => YouTubePlaylistProbeStatus {
            read_succeeded: true,
            playlist_count: playlists.len(),
            result: "read_succeeded",
        },
        Err(_) => YouTubePlaylistProbeStatus {
            read_succeeded: false,
            playlist_count: 0,
            result: "read_failed",
        },
    }
}

/// Turns one `browse` payload of a playlist (embedded in its page or fetched by `browse`) into the
/// page the interface shows, and records the continuation token for the next page.
#[cfg(windows)]
fn playlist_page_from_payload(
    app: &tauri::AppHandle,
    playlist_id: &str,
    list_id: &str,
    payload: &serde_json::Value,
) -> Option<YouTubePlaylistPage> {
    let tracks = playlist_tracks_from_payload(payload);
    if tracks.is_empty() {
        return None;
    }
    let has_more = playlist_continuation_for(payload, list_id).is_some();
    let continuations = YOUTUBE_PLAYLIST_CONTINUATIONS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut continuations) = continuations.lock() {
        if let Some(token) = playlist_continuation_for(payload, list_id) {
            continuations.insert(playlist_id.to_string(), token);
            if let Ok(mut seen) = YOUTUBE_PLAYLIST_SEEN_CONTINUATIONS
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
            {
                let mut tokens = HashSet::new();
                tokens.insert(continuations[playlist_id].clone());
                seen.insert(playlist_id.to_string(), tokens);
            }
        } else {
            // An empty marker distinguishes a terminal page from bridge state.
            continuations.insert(playlist_id.to_string(), String::new());
        }
    }
    write_native_log(
        app,
        &format!(
            "YouTube playlist page returned {} tracks: playlist_id={playlist_id}",
            tracks.len()
        ),
    );
    Some(YouTubePlaylistPage {
        tracks,
        has_more,
        continuation: playlist_continuation_state(playlist_id),
    })
}

#[cfg(windows)]
#[tauri::command]
async fn load_youtube_playlist_tracks(
    app: tauri::AppHandle,
    playlist_id: String,
) -> Result<YouTubePlaylistPage, String> {
    use tauri::Manager;
    let _library_lock = YOUTUBE_LIBRARY_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    write_native_log(
        &app,
        &format!("Loading YouTube Music playlist tracks: {playlist_id}"),
    );
    let list_id = playlist_id.strip_prefix("VL").unwrap_or(&playlist_id);
    let session_path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("youtube-session.bin");
    let session = YouTubeSession::load(&session_path)?;
    let endpoint = format!("https://music.youtube.com/playlist?list={list_id}");
    let cookie_header = session.cookie_header_for(&endpoint, unix_now()?)?;
    let html = youtube_http::client()?
        .get(endpoint)
        .header("Cookie", cookie_header)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153 Safari/537.36",
        )
        .send()
        .await
        .map_err(|error| error.to_string())?
        .text()
        .await
        .map_err(|error| error.to_string())?;
    let embedded = regex::Regex::new(
        r#"initialData\.push\(\{\s*path:\s*['"]\\?/browse['"][\s\S]*?data:\s*'((?:\\.|[^'])*)'"#,
    )
    .map_err(|error| error.to_string())?;
    for captures in embedded.captures_iter(&html) {
        let Some(decoded) = captures
            .get(1)
            .and_then(|value| decode_javascript_string(value.as_str()))
        else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(&decoded) else {
            continue;
        };
        if let Some(page) = playlist_page_from_payload(&app, &playlist_id, list_id, &payload) {
            return Ok(page);
        }
    }
    match youtube_browse::BrowseSession::open(&app).await {
        Ok(browse) => {
            let referer = format!("https://music.youtube.com/playlist?list={list_id}");
            match browse.browse(&format!("VL{list_id}"), &referer).await {
                Ok(payload) => {
                    if let Some(page) =
                        playlist_page_from_payload(&app, &playlist_id, list_id, &payload)
                    {
                        eprintln!("VibeDeck playlist path=native_browse playlist={playlist_id}");
                        return Ok(page);
                    }
                    write_native_log(&app, &format!("YouTube native browse returned no tracks: playlist_id={playlist_id}"));
                }
                Err(code) => write_native_log(
                    &app,
                    &format!("YouTube native browse failed ({code}): playlist_id={playlist_id}"),
                ),
            }
        }
        Err(code) => write_native_log(
            &app,
            &format!("YouTube native browse unavailable ({code}): playlist_id={playlist_id}"),
        ),
    }
    Err("Nao foi possivel carregar as faixas da playlist.".to_string())
}

#[cfg(windows)]
#[tauri::command]
async fn load_youtube_playlist_more_tracks(
    app: tauri::AppHandle,
    playlist_id: String,
) -> Result<YouTubePlaylistPage, String> {
    use tauri::Manager;
    let _library_lock = YOUTUBE_LIBRARY_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    write_native_log(
        &app,
        &format!("Loading more YouTube Music playlist tracks: {playlist_id}"),
    );
    let memory_token = YOUTUBE_PLAYLIST_CONTINUATIONS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|error| error.to_string())?
        .get(&playlist_id)
        .cloned();
    let token = memory_token.or_else(|| {
        playlist_catalog::continuation_for_playlist(&app, &playlist_id)
            .ok()
            .flatten()
    });
    if let Some(token) = token {
        if token.is_empty() {
            write_native_log(
                &app,
                &format!("YouTube playlist has no continuation: playlist_id={playlist_id}"),
            );
            return Ok(YouTubePlaylistPage {
                tracks: Vec::new(),
                has_more: false,
                continuation: None,
            });
        }
        YOUTUBE_PLAYLIST_SEEN_CONTINUATIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?
            .entry(playlist_id.clone())
            .or_default()
            .insert(token.clone());
        YOUTUBE_PLAYLIST_CONTINUATIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?
            .insert(playlist_id.clone(), token.clone());
        let list_id = playlist_id.strip_prefix("VL").unwrap_or(&playlist_id);
        let session_path = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?
            .join("youtube-session.bin");
        let session = YouTubeSession::load(&session_path)?;
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153.0.0.0 Safari/537.36";
        let client = youtube_http::client()?;
        let config_html = client
            .get("https://music.youtube.com")
            .header(
                "Cookie",
                session.cookie_header_for("https://music.youtube.com/", unix_now()?)?,
            )
            .header("User-Agent", user_agent)
            .send()
            .await
            .map_err(|error| error.to_string())?
            .text()
            .await
            .map_err(|error| error.to_string())?;
        let read_config = |name: &str| {
            regex::Regex::new(&format!(r#"{name}":"([^\"]+)""#))
                .ok()
                .and_then(|pattern| pattern.captures(&config_html))
                .and_then(|captures| captures.get(1))
                .map(|value| value.as_str().to_string())
                .unwrap_or_default()
        };
        let api_key = read_config("INNERTUBE_API_KEY");
        let mut client_version = read_config("INNERTUBE_CLIENT_VERSION");
        if client_version.is_empty() {
            client_version = "1.20260920.16.00".to_string();
        }
        let visitor_id = read_config("VISITOR_DATA");
        write_native_log(&app, &format!("YouTube continuation config: client_version={client_version}, visitor={}, api_key={}", !visitor_id.is_empty(), !api_key.is_empty()));
        let body = serde_json::json!({ "context": { "client": { "clientName": "WEB_REMIX", "clientVersion": client_version, "hl": "pt", "gl": "BR", "visitorData": visitor_id, "platform": "DESKTOP", "clientFormFactor": "UNKNOWN_FORM_FACTOR" }, "user": { "lockedSafetyMode": false }, "request": { "useSsl": true } }, "continuation": token });
        let mut endpoint = url::Url::parse("https://music.youtube.com/youtubei/v1/browse")
            .map_err(|error| error.to_string())?;
        {
            let mut query = endpoint.query_pairs_mut();
            query
                .append_pair("ctoken", &token)
                .append_pair("continuation", &token)
                .append_pair("type", "next")
                .append_pair("prettyPrint", "false");
            if !api_key.is_empty() {
                query.append_pair("key", &api_key);
            }
        }
        let timestamp = unix_now()?;
        let cookie_header = session.cookie_header_for(endpoint.as_str(), timestamp)?;
        let authorization = session.authorization(endpoint.as_str(), timestamp as u64, None)?;
        let response = client
            .post(endpoint)
            .header("Authorization", authorization)
            .header("Cookie", cookie_header)
            .header("X-Youtube-Client-Name", "67")
            .header("X-Youtube-Client-Version", &client_version)
            .header("X-Goog-Visitor-Id", visitor_id)
            .header("X-Origin", "https://music.youtube.com")
            .header("X-Goog-AuthUser", "0")
            .header("X-Youtube-Bootstrap-Logged-In", "true")
            .header("X-Goog-Request-Time", timestamp.to_string())
            .header(
                "Referer",
                format!("https://music.youtube.com/playlist?list={list_id}"),
            )
            .header("User-Agent", user_agent)
            .header("Origin", "https://music.youtube.com")
            .json(&body)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let payload = response
            .json::<serde_json::Value>()
            .await
            .map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!("YouTube continuation HTTP {status}: {payload}"));
        }
        let tracks = playlist_tracks_from_payload(&payload);
        let mut continuations = YOUTUBE_PLAYLIST_CONTINUATIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .map_err(|error| error.to_string())?;
        if let Some(next) = playlist_continuation_for(&payload, list_id) {
            let repeated = YOUTUBE_PLAYLIST_SEEN_CONTINUATIONS
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .map_err(|error| error.to_string())?
                .entry(playlist_id.clone())
                .or_default()
                .contains(&next);
            if repeated {
                continuations.remove(&playlist_id);
            } else {
                continuations.insert(playlist_id.clone(), next.clone());
                YOUTUBE_PLAYLIST_SEEN_CONTINUATIONS
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock()
                    .map_err(|error| error.to_string())?
                    .entry(playlist_id.clone())
                    .or_default()
                    .insert(next);
            }
        } else {
            continuations.remove(&playlist_id);
        }
        write_native_log(
            &app,
            &format!(
                "YouTube direct continuation returned {} tracks: playlist_id={playlist_id}",
                tracks.len()
            ),
        );
        let has_more = continuations.contains_key(&playlist_id);
        drop(continuations);
        let continuation = playlist_continuation_state(&playlist_id);
        return Ok(YouTubePlaylistPage {
            tracks,
            has_more,
            continuation,
        });
    }
    // Without a stored token there is nothing more to fetch: the first page already
    // recorded either the next token or an empty marker.
    Ok(YouTubePlaylistPage {
        tracks: Vec::new(),
        has_more: false,
        continuation: None,
    })
}

#[cfg(windows)]
#[derive(Clone)]
struct CapturedUmpResponse {
    status: i32,
    request_method: String,
    content_type: String,
    request_uri: String,
    request_headers: Option<audio::AudioRequestHeaderSummary>,
    request_body: Option<audio::AudioRequestBodySummary>,
    body: Vec<u8>,
    capture_error: Option<&'static str>,
}

#[cfg(windows)]
fn webview_request_header_summary(
    request: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2WebResourceRequest,
) -> Option<audio::AudioRequestHeaderSummary> {
    let webview_headers = unsafe { request.Headers() }.ok()?;
    let mut safe_headers = reqwest::header::HeaderMap::new();

    for name in [
        "Origin",
        "Referer",
        "User-Agent",
        "Content-Type",
        "Content-Length",
        "Accept",
        "Accept-Encoding",
        "Sec-Fetch-Site",
    ] {
        let header_name = webview2_com::CoTaskMemPWSTR::from(name);
        let mut raw_value = windows_core::PWSTR::null();
        if unsafe { webview_headers.GetHeader(*header_name.as_ref().as_pcwstr(), &mut raw_value) }
            .is_ok()
        {
            let value = webview2_com::CoTaskMemPWSTR::from(raw_value).to_string();
            if let (Ok(name), Ok(value)) = (
                reqwest::header::HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes()),
                reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
            ) {
                safe_headers.insert(name, value);
            }
        }
    }

    for name in [
        "Cookie",
        "Authorization",
        "Sec-CH-UA",
        "Sec-CH-UA-Mobile",
        "Sec-CH-UA-Platform",
        "X-Goog-Visitor-Id",
        "X-Goog-AuthUser",
    ] {
        let header_name = webview2_com::CoTaskMemPWSTR::from(name);
        let mut raw_value = windows_core::PWSTR::null();
        if unsafe { webview_headers.GetHeader(*header_name.as_ref().as_pcwstr(), &mut raw_value) }
            .is_ok()
        {
            // Free the COM string without converting, retaining, or logging its value.
            drop(webview2_com::CoTaskMemPWSTR::from(raw_value));
            if let Ok(name) =
                reqwest::header::HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes())
            {
                safe_headers.insert(name, reqwest::header::HeaderValue::from_static("present"));
            }
        }
    }

    if let Ok(iterator) = unsafe { webview_headers.GetIterator() } {
        let mut has_current = windows_core::BOOL::default();
        let mut collected = 0;
        while collected < 64
            && unsafe { iterator.HasCurrentHeader(&mut has_current as *mut _) }.is_ok()
            && has_current.0 != 0
        {
            let mut raw_name = windows_core::PWSTR::null();
            let mut raw_value = windows_core::PWSTR::null();
            if unsafe { iterator.GetCurrentHeader(&mut raw_name, &mut raw_value) }.is_err() {
                break;
            }
            let name = webview2_com::CoTaskMemPWSTR::from(raw_name)
                .to_string()
                .to_ascii_lowercase();
            // Discard every value from enumeration; only header names are retained.
            drop(webview2_com::CoTaskMemPWSTR::from(raw_value));
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                if !safe_headers.contains_key(&name) {
                    safe_headers
                        .insert(name, reqwest::header::HeaderValue::from_static("observed"));
                }
            }
            collected += 1;
            let mut has_next = windows_core::BOOL::default();
            if unsafe { iterator.MoveNext(&mut has_next as *mut _) }.is_err() || has_next.0 == 0 {
                break;
            }
        }
    }

    Some(audio::audio_request_header_summary(&safe_headers))
}

#[cfg(windows)]
fn webview_request_body_summary(
    request: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2WebResourceRequest,
) -> Option<audio::AudioRequestBodySummary> {
    use windows::Win32::System::Com::STREAM_SEEK_SET;

    let stream = unsafe { request.Content() }.ok()?;
    let clone = unsafe { stream.Clone() }.ok()?;
    unsafe { clone.Seek(0, STREAM_SEEK_SET, None) }.ok()?;

    // Read at most 65 bytes and retain only a fixed body-shape category.
    let mut bytes = [0_u8; 65];
    let mut read = 0_u32;
    let result = unsafe {
        clone.Read(
            bytes.as_mut_ptr().cast(),
            bytes.len() as u32,
            Some(&mut read),
        )
    };
    if result.is_err() {
        return Some(audio::AudioRequestBodySummary {
            byte_length: None,
            category: "unavailable".into(),
        });
    }
    let body = &bytes[..read as usize];
    let category = match body {
        b"x" => "x",
        b"x=" => "x_equals",
        b"x\0" => "x_nul",
        b"x " => "x_space",
        b"x\r" => "x_cr",
        b"x\n" => "x_lf",
        [] => "empty",
        bytes if bytes.iter().all(|byte| (0x20..=0x7e).contains(byte)) => "printable_ascii",
        bytes if bytes.iter().all(|byte| *byte < 0x80) => "ascii_control_or_text",
        _ => "non_ascii",
    };
    Some(audio::AudioRequestBodySummary {
        byte_length: (read < bytes.len() as u32).then_some(read as u64),
        category: category.into(),
    })
}

#[cfg(windows)]
fn sanitize_ump_query_parameter_names(parsed: &url::Url) -> Vec<String> {
    const MAX_NAMES: usize = 24;
    const MAX_NAME_LENGTH: usize = 32;

    let mut names = std::collections::BTreeSet::new();
    for (name, _) in parsed.query_pairs() {
        let sanitized: String = name
            .chars()
            .take(MAX_NAME_LENGTH)
            .filter(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
            })
            .collect();
        if !sanitized.is_empty() {
            names.insert(sanitized);
            if names.len() >= MAX_NAMES {
                break;
            }
        }
    }
    names.into_iter().collect()
}

#[cfg(windows)]
fn ump_response_content_type_category(content_type: Option<&str>) -> &'static str {
    match content_type {
        None => "absent",
        Some(value)
            if value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/vnd.yt-ump")) =>
        {
            "ump"
        }
        Some(_) => "other",
    }
}

#[cfg(windows)]
fn ump_response_rejection_reason(
    method_is_post: bool,
    host_is_googlevideo: bool,
    path_is_videoplayback: bool,
    status: Option<i32>,
    content_type_category: &str,
) -> &'static str {
    if !method_is_post {
        "not_post"
    } else if !host_is_googlevideo {
        "host_mismatch"
    } else if !path_is_videoplayback {
        "path_mismatch"
    } else if status.is_none() {
        "status_unavailable"
    } else if !status.is_some_and(|value| (200..300).contains(&value)) {
        "non_success_status"
    } else if content_type_category == "absent" {
        "content_type_absent"
    } else if content_type_category != "ump" {
        "non_ump_content_type"
    } else {
        "accepted"
    }
}

#[cfg(windows)]
fn reserve_ump_diagnostic_slot(counter: &AtomicUsize) -> bool {
    const MAX_DIAGNOSTICS_PER_ACTIVATION: usize = 16;
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_DIAGNOSTICS_PER_ACTIVATION).then_some(count + 1)
        })
        .is_ok()
}

#[cfg(windows)]
fn is_successful_capture(response: &Result<CapturedUmpResponse, &'static str>) -> bool {
    response
        .as_ref()
        .is_ok_and(|capture| capture.capture_error.is_none())
}

#[cfg(windows)]
fn is_captured_audio_ump_response(response: &CapturedUmpResponse) -> bool {
    response.capture_error.is_none()
        && is_acceptable_ump_response(
            &response.request_method,
            &response.request_uri,
            response.status,
            &response.content_type,
        )
}

#[cfg(windows)]
fn is_acceptable_ump_response(method: &str, uri: &str, status: i32, content_type: &str) -> bool {
    if !method.eq_ignore_ascii_case("POST") || !(200..300).contains(&status) {
        return false;
    }
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/vnd.yt-ump"))
    {
        return false;
    }
    let Ok(parsed) = url::Url::parse(uri) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.path() == "/videoplayback"
        && parsed.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("googlevideo.com")
                || host.to_ascii_lowercase().ends_with(".googlevideo.com")
        })
}

#[cfg(windows)]
fn send_with_timeout<T>(
    sender: &std::sync::mpsc::SyncSender<T>,
    mut value: T,
    timeout: std::time::Duration,
) -> Result<(), T> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match sender.try_send(value) {
            Ok(()) => return Ok(()),
            Err(std::sync::mpsc::TrySendError::Full(returned)) => {
                if std::time::Instant::now() >= deadline {
                    return Err(returned);
                }
                value = returned;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(returned)) => return Err(returned),
        }
    }
}

#[cfg(windows)]
fn send_captured_ump_response(
    sender: &std::sync::mpsc::SyncSender<Result<CapturedUmpResponse, &'static str>>,
    response: CapturedUmpResponse,
    continuous: bool,
    app: &tauri::AppHandle,
) {
    if continuous {
        if send_with_timeout(sender, Ok(response), std::time::Duration::from_secs(3)).is_err() {
            write_native_log(
                app,
                "YouTube UMP relay capture queue is full; response dropped",
            );
        }
    } else {
        let _ = sender.try_send(Ok(response));
    }
}

#[cfg(windows)]
fn register_ump_response_capture(
    window: &tauri::WebviewWindow,
    activation_sequence: u64,
    continuous: bool,
) -> Result<
    (
        std::sync::mpsc::Receiver<Result<CapturedUmpResponse, &'static str>>,
        std::sync::Arc<Mutex<Option<i64>>>,
    ),
    String,
> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;

    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let channel_capacity = if continuous { 4 } else { 1 };
    let (sender, receiver) = std::sync::mpsc::sync_channel(channel_capacity);
    let token = std::sync::Arc::new(Mutex::new(None));
    let callback_token = token.clone();
    let callback_sender = sender.clone();
    let capture_claimed = std::sync::Arc::new(AtomicBool::new(false));
    let callback_capture_claimed = capture_claimed.clone();
    let diagnostic_count = std::sync::Arc::new(AtomicUsize::new(0));
    let callback_diagnostic_count = diagnostic_count.clone();
    let callback_app = window.app_handle().clone();
    let callback_error = std::sync::Arc::new(Mutex::new(None::<String>));
    let setup_error = callback_error.clone();

    window
        .with_webview(move |webview| {
            let core = unsafe { webview.controller().CoreWebView2() }
                .and_then(|core| core.cast::<ICoreWebView2_2>());
            let core = match core {
                Ok(core) => core,
                Err(error) => {
                    if let Ok(mut stored) = setup_error.lock() {
                        *stored = Some(error.to_string());
                    }
                    return;
                }
            };

            let handler = webview2_com::WebResourceResponseReceivedEventHandler::create(Box::new(
                move |_sender, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let request = match unsafe { args.Request() } {
                        Ok(request) => request,
                        Err(_) => return Ok(()),
                    };
                    let mut raw_uri = windows_core::PWSTR::null();
                    let mut raw_method = windows_core::PWSTR::null();
                    if unsafe { request.Uri(&mut raw_uri) }.is_err()
                        || unsafe { request.Method(&mut raw_method) }.is_err()
                    {
                        return Ok(());
                    }
                    let uri = webview2_com::CoTaskMemPWSTR::from(raw_uri).to_string();
                    let method = webview2_com::CoTaskMemPWSTR::from(raw_method).to_string();
                    let parsed = match url::Url::parse(&uri) {
                        Ok(parsed) => parsed,
                        Err(_) => return Ok(()),
                    };
                    let method_is_post = method.eq_ignore_ascii_case("POST");
                    let host_is_googlevideo = parsed.host_str().is_some_and(|host| {
                        host.eq_ignore_ascii_case("googlevideo.com")
                            || host.to_ascii_lowercase().ends_with(".googlevideo.com")
                    });
                    let path_is_videoplayback = parsed.path() == "/videoplayback";
                    let is_diagnostic_candidate = method_is_post
                        && host_is_googlevideo
                        && path_is_videoplayback;
                    if !is_diagnostic_candidate {
                        return Ok(());
                    }
                    let query_names = sanitize_ump_query_parameter_names(&parsed);
                    let has_ump_flag = parsed
                        .query_pairs()
                        .any(|(key, value)| key == "ump" && value == "1");
                    let has_audio_mime_marker = matches!(
                        audio::audio_request_context(&parsed, "POST")
                            .mime_type
                            .as_str(),
                        "audio/mp4" | "audio/webm"
                    );
                    let is_ump_request = method.eq_ignore_ascii_case("POST")
                        && parsed.host_str().is_some_and(|host| {
                            host.to_ascii_lowercase().ends_with("googlevideo.com")
                        })
                        && parsed.path() == "/videoplayback";
                    let (request_headers, request_body) = if is_ump_request {
                        (
                            webview_request_header_summary(&request),
                            webview_request_body_summary(&request),
                        )
                    } else {
                        (None, None)
                    };
                    let response = match unsafe { args.Response() } {
                        Ok(response) => response,
                        Err(_) => {
                            if reserve_ump_diagnostic_slot(&callback_diagnostic_count) {
                                let rejection_reason = ump_response_rejection_reason(
                                    method_is_post,
                                    host_is_googlevideo,
                                    path_is_videoplayback,
                                    None,
                                    "absent",
                                );
                                write_native_log(
                                    &callback_app,
                                    &format!(
                                        "YouTube UMP prefilter activation={} method=post host_path=googlevideo_videoplayback query_names={:?} status=absent content_type=absent has_ump_flag={} has_audio_mime_marker={} accepted=false rejection_reason={}",
                                        activation_sequence, query_names, has_ump_flag, has_audio_mime_marker, rejection_reason
                                    ),
                                );
                            }
                            return Ok(());
                        }
                    };
                    let mut status = 0_i32;
                    if unsafe { response.StatusCode(&mut status) }.is_err() {
                        if reserve_ump_diagnostic_slot(&callback_diagnostic_count) {
                            write_native_log(
                                &callback_app,
                                &format!(
                                    "YouTube UMP prefilter activation={} method=post host_path=googlevideo_videoplayback query_names={:?} status=absent content_type=absent has_ump_flag={} has_audio_mime_marker={} accepted=false rejection_reason={}",
                                    activation_sequence,
                                    query_names,
                                    has_ump_flag,
                                    has_audio_mime_marker,
                                    ump_response_rejection_reason(
                                        method_is_post,
                                        host_is_googlevideo,
                                        path_is_videoplayback,
                                        None,
                                        "absent"
                                    )
                                ),
                            );
                        }
                        return Ok(());
                    }
                    let headers = match unsafe { response.Headers() } {
                        Ok(headers) => headers,
                        Err(_) => {
                            if reserve_ump_diagnostic_slot(&callback_diagnostic_count) {
                                let rejection_reason = ump_response_rejection_reason(
                                    method_is_post,
                                    host_is_googlevideo,
                                    path_is_videoplayback,
                                    Some(status),
                                    "absent",
                                );
                                write_native_log(
                                    &callback_app,
                                    &format!(
                                        "YouTube UMP prefilter activation={} method=post host_path=googlevideo_videoplayback query_names={:?} status={} content_type=absent has_ump_flag={} has_audio_mime_marker={} accepted=false rejection_reason={}",
                                        activation_sequence, query_names, status, has_ump_flag, has_audio_mime_marker, rejection_reason
                                    ),
                                );
                            }
                            return Ok(());
                        }
                    };
                    let header_name = webview2_com::CoTaskMemPWSTR::from("Content-Type");
                    let mut raw_content_type = windows_core::PWSTR::null();
                    if unsafe {
                        headers.GetHeader(*header_name.as_ref().as_pcwstr(), &mut raw_content_type)
                    }
                    .is_err()
                    {
                        if reserve_ump_diagnostic_slot(&callback_diagnostic_count) {
                            let rejection_reason = ump_response_rejection_reason(
                                method_is_post,
                                host_is_googlevideo,
                                path_is_videoplayback,
                                Some(status),
                                "absent",
                            );
                            write_native_log(
                                &callback_app,
                                &format!(
                                    "YouTube UMP prefilter activation={} method=post host_path=googlevideo_videoplayback query_names={:?} status={} content_type=absent has_ump_flag={} has_audio_mime_marker={} accepted=false rejection_reason={}",
                                    activation_sequence, query_names, status, has_ump_flag, has_audio_mime_marker, rejection_reason
                                ),
                            );
                        }
                        return Ok(());
                    }
                    let content_type =
                        webview2_com::CoTaskMemPWSTR::from(raw_content_type).to_string();
                    let content_type_category =
                        ump_response_content_type_category(Some(&content_type));
                    let rejection_reason = ump_response_rejection_reason(
                        method_is_post,
                        host_is_googlevideo,
                        path_is_videoplayback,
                        Some(status),
                        content_type_category,
                    );
                    if reserve_ump_diagnostic_slot(&callback_diagnostic_count) {
                        write_native_log(
                            &callback_app,
                            &format!(
                                "YouTube UMP prefilter activation={} method=post host_path=googlevideo_videoplayback query_names={:?} status={} content_type={} has_ump_flag={} has_audio_mime_marker={} accepted={} rejection_reason={}",
                                activation_sequence,
                                query_names,
                                status,
                                content_type_category,
                                has_ump_flag,
                                has_audio_mime_marker,
                                rejection_reason == "accepted",
                                rejection_reason
                            ),
                        );
                    }
                    if !is_acceptable_ump_response(&method, &uri, status, &content_type) {
                        return Ok(());
                    }
                    if !continuous && callback_capture_claimed.swap(true, Ordering::AcqRel) {
                        return Ok(());
                    }
                    if let Ok(mut snapshot) = UMP_CAPTURE_DIAGNOSTICS
                        .get_or_init(|| {
                            Mutex::new(UmpCaptureSnapshot {
                                scope: "webview_capture_only_not_audio_engine_input_proof",
                                stage: "capture",
                                outcome: "in_progress",
                                activation_sequence: Some(activation_sequence),
                                http_status: None,
                                content_type: None,
                                same_stream_request: None,
                                used_capture_as_playback_url: None,
                                request_context: None,
                                request_headers: None,
                                request_body: None,
                                rust_parser_outcome: None,
                                rust_extracted_media_bytes: None,
                                body_bytes: 0,
                                media_bytes: 0,
                                part_kinds: Vec::new(),
                            })
                        })
                        .lock()
                    {
                        ump_capture_response_observed(
                            &mut snapshot,
                            status,
                            &content_type,
                            &uri,
                            request_headers.clone(),
                            request_body.clone(),
                        );
                    }

                    let body_sender = callback_sender.clone();
                    let body_capture_claimed = callback_capture_claimed.clone();
                    let completed_app = callback_app.clone();
                    let callback_content_type = content_type.clone();
                    let request_uri = uri.clone();
                    let callback_method = method.clone();
                    let callback_request_headers = request_headers.clone();
                    let callback_request_body = request_body.clone();
                    let completed =
                        webview2_com::WebResourceResponseViewGetContentCompletedHandler::create(
                            Box::new(move |error_code, stream| {
                                let failure = |reason| CapturedUmpResponse {
                                    status,
                                    request_method: callback_method.clone(),
                                    content_type: callback_content_type.clone(),
                                    request_uri: request_uri.clone(),
                                    request_headers: callback_request_headers.clone(),
                                    request_body: callback_request_body.clone(),
                                    body: Vec::new(),
                                    capture_error: Some(reason),
                                };
                                let fail = |reason: &'static str| {
                                    if !continuous {
                                        body_capture_claimed.store(false, Ordering::Release);
                                    }
                                    send_captured_ump_response(
                                        &body_sender,
                                        failure(reason),
                                        continuous,
                                        &completed_app,
                                    );
                                };
                                if error_code.is_err() {
                                    fail("get_content_failed");
                                    return Ok(());
                                }
                                let Some(stream) = stream else {
                                    fail("content_stream_unavailable");
                                    return Ok(());
                                };
                                let mut body = Vec::with_capacity(64 * 1024);
                                loop {
                                    let remaining =
                                        MAX_BODY_BYTES.saturating_add(1).saturating_sub(body.len());
                                    if remaining == 0 {
                                        fail("response_too_large");
                                        return Ok(());
                                    }
                                    let mut chunk = vec![0_u8; remaining.min(64 * 1024)];
                                    let mut bytes_read = 0_u32;
                                    let read_result = unsafe {
                                        stream.Read(
                                            chunk.as_mut_ptr().cast(),
                                            chunk.len() as u32,
                                            Some(&mut bytes_read),
                                        )
                                    };
                                    if read_result.is_err() {
                                        fail("content_stream_read_failed");
                                        return Ok(());
                                    }
                                    if bytes_read == 0 {
                                        break;
                                    }
                                    body.extend_from_slice(&chunk[..bytes_read as usize]);
                                    if body.len() > MAX_BODY_BYTES {
                                        fail("response_too_large");
                                        return Ok(());
                                    }
                                }
                                send_captured_ump_response(
                                    &body_sender,
                                    CapturedUmpResponse {
                                        status,
                                        request_method: callback_method,
                                        content_type: callback_content_type,
                                        request_uri,
                                        request_headers: callback_request_headers,
                                        request_body: callback_request_body,
                                        body,
                                        capture_error: None,
                                    },
                                    continuous,
                                    &completed_app,
                                );
                                Ok(())
                            }),
                        );
                    if unsafe { response.GetContent(&completed) }.is_err() {
                        if !continuous {
                            callback_capture_claimed.store(false, Ordering::Release);
                        }
                        send_captured_ump_response(
                            &callback_sender,
                            CapturedUmpResponse {
                                status,
                                request_method: method,
                                content_type,
                                request_uri: uri,
                                request_headers,
                                request_body,
                                body: Vec::new(),
                                capture_error: Some("get_content_start_failed"),
                            },
                            continuous,
                            &callback_app,
                        );
                    }
                    Ok(())
                },
            ));

            let mut event_token = 0_i64;
            if let Err(error) =
                unsafe { core.add_WebResourceResponseReceived(&handler, &mut event_token) }
            {
                if let Ok(mut stored) = setup_error.lock() {
                    *stored = Some(error.to_string());
                }
                return;
            }
            if let Ok(mut stored) = callback_token.lock() {
                *stored = Some(event_token);
            }
        })
        .map_err(|error| error.to_string())?;

    if let Some(error) = callback_error
        .lock()
        .map_err(|error| error.to_string())?
        .take()
    {
        return Err(error);
    }
    Ok((receiver, token))
}

#[cfg(windows)]
fn unregister_ump_response_capture(
    window: &tauri::WebviewWindow,
    token: &std::sync::Arc<Mutex<Option<i64>>>,
) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;

    let Some(token) = token.lock().map_err(|error| error.to_string())?.take() else {
        return Ok(());
    };
    let result = std::sync::Arc::new(Mutex::new(None::<String>));
    let callback_result = result.clone();
    window
        .with_webview(move |webview| {
            let remove_result = unsafe { webview.controller().CoreWebView2() }
                .and_then(|core| core.cast::<ICoreWebView2_2>())
                .and_then(|core| unsafe { core.remove_WebResourceResponseReceived(token) });
            if let Err(error) = remove_result {
                if let Ok(mut stored) = callback_result.lock() {
                    *stored = Some(error.to_string());
                }
            }
        })
        .map_err(|error| error.to_string())?;
    let cleanup_error = result
        .lock()
        .map_err(|error| error.to_string())?
        .take()
        .map_or(Ok(()), Err);
    cleanup_error
}

#[cfg(windows)]
fn summarize_ump_capture(body: &[u8]) -> (Vec<UmpPartKindCount>, u64) {
    fn read_varint(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
        let first = *bytes.get(*cursor)?;
        *cursor += 1;
        let extra = match first >> 4 {
            0..=7 => 0,
            8..=11 => 1,
            12..=13 => 2,
            14 => 3,
            _ => return None,
        };
        let mask = match extra {
            0 => 0x7f,
            1 => 0x3f,
            2 => 0x1f,
            _ => 0x0f,
        };
        let mut value = u32::from(first & mask);
        let shifts: &[u32] = match extra {
            0 => &[],
            1 => &[6],
            2 => &[5, 13],
            _ => &[4, 12, 20],
        };
        for shift in shifts {
            let next = u32::from(*bytes.get(*cursor)?);
            *cursor += 1;
            value |= next << *shift;
        }
        Some(value)
    }

    let mut cursor = 0;
    let mut counts = std::collections::BTreeMap::<u32, u32>::new();
    let mut media_bytes = 0_u64;
    while cursor < body.len() {
        let Some(kind) = read_varint(body, &mut cursor) else {
            break;
        };
        let Some(length) = read_varint(body, &mut cursor).map(|length| length as usize) else {
            break;
        };
        let Some(end) = cursor.checked_add(length).filter(|end| *end <= body.len()) else {
            break;
        };
        *counts.entry(kind).or_default() += 1;
        if kind == 21 {
            media_bytes = media_bytes.saturating_add(length.saturating_sub(1) as u64);
        }
        cursor = end;
    }
    let parts = counts
        .into_iter()
        .take(32)
        .map(|(kind, count)| UmpPartKindCount { kind, count })
        .collect();
    (parts, media_bytes)
}

#[cfg(windows)]
async fn ensure_youtube_stream_bridge(
    app: &tauri::AppHandle,
) -> Result<tauri::WebviewWindow, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    let session_path = app
        .path()
        .app_data_dir()
        .map_err(|_| "youtube_session_directory_failed")?
        .join("youtube-session.bin");
    let session = YouTubeSession::load(&session_path)?;
    if let Some(window) = app.get_webview_window("youtube-stream-bridge") {
        return Ok(window);
    }

    let window = WebviewWindowBuilder::new(
        app,
        "youtube-stream-bridge",
        WebviewUrl::App("index.html".into()),
    )
    .title("VibeDeck - YouTube Music stream bridge")
    .visible(false)
    .initialization_script(r#"(() => { window.addEventListener('beforeunload', (event) => event.stopImmediatePropagation(), true); const silence = (media) => { if (!(media instanceof HTMLMediaElement)) return; media.muted = true; media.defaultMuted = true; media.volume = 0; }; const play = HTMLMediaElement.prototype.play; HTMLMediaElement.prototype.play = function(...args) { silence(this); return play.apply(this, args); }; new MutationObserver((records) => records.forEach((record) => record.addedNodes.forEach((node) => { if (node instanceof HTMLMediaElement) silence(node); if (node instanceof Element) node.querySelectorAll?.('audio, video').forEach(silence); }))).observe(document, { childList: true, subtree: true }); document.querySelectorAll?.('audio, video').forEach(silence); })();"#)
    .on_page_load(|window, payload| {
        let is_youtube_watch_page = payload.url().host_str() == Some("music.youtube.com")
            && payload.url().path() == "/watch";
        write_native_log(
            window.app_handle(),
            &format!(
                "YouTube stream bridge page: event={:?} youtube_watch_page={is_youtube_watch_page}",
                payload.event()
            ),
        );
        if is_youtube_watch_page {
            let script = r#"(() => { const isAudio = (url) => /(?:mime=audio|mime=audio%2F|type=audio|audio_only)/i.test(url) && !/mime=video/i.test(url); let attempts = 0; let announcedUrl = false; let playing = false; const hook = (video) => { if (video.__vibedeckHooked) return; video.__vibedeckHooked = true; video.addEventListener('ended', () => { document.title = '__VIBEDECK_STREAM_ENDED__'; }, { once: true }); video.addEventListener('error', () => { document.title = '__VIBEDECK_STREAM_ERROR__'; }, { once: true }); }; const collect = () => { if (!announcedUrl) { const entries = performance.getEntriesByType('resource').slice().reverse(); const entry = entries.find((item) => item.name.includes('googlevideo.com/videoplayback') && isAudio(item.name)); if (entry) { announcedUrl = true; document.title = '__VIBEDECK_STREAM__' + encodeURIComponent(entry.name); } } const video = document.querySelector('video'); if (video) { hook(video); video.muted = true; video.defaultMuted = true; video.volume = 0; if (!playing) { const attempt = video.play(); if (attempt && attempt.then) { attempt.then(() => { playing = true; }).catch(() => {}); } else { playing = true; } } } if (attempts++ < 100) setTimeout(collect, 150); }; collect(); setInterval(() => { const video = document.querySelector('video'); if (video) hook(video); }, 2000); })();"#;
            let _ = window.eval(script);
        }
    })
    .on_document_title_changed(|_, title| {
        if let Some(encoded) = title.strip_prefix("__VIBEDECK_STREAM__") {
            if let Some((_, value)) = url::form_urlencoded::parse(format!("value={encoded}").as_bytes()).next() {
                if let Some(result) = YOUTUBE_STREAM_BRIDGE.get() {
                    if let Ok(mut stored) = result.lock() { *stored = Some(value.into_owned()); }
                }
            }
        } else if title == "__VIBEDECK_STREAM_ENDED__" {
            UMP_RELAY_PAGE_ENDED.store(true, Ordering::Release);
        } else if title == "__VIBEDECK_STREAM_ERROR__" {
            UMP_RELAY_PAGE_ERROR.store(true, Ordering::Release);
        }
    })
    .build()
    .map_err(|error| error.to_string())?;
    if let Err(error) = youtube_cookie_bridge::restore(&window, &session).await {
        let _ = window.close();
        return Err(error);
    }
    Ok(window)
}

#[cfg(windows)]
async fn resolve_youtube_audio_bridge_inner(
    app: tauri::AppHandle,
    video_id: String,
    activation_sequence: u64,
    first_response_only: bool,
) -> Result<(serde_json::Value, Option<CapturedUmpResponse>), String> {
    ump_capture_progress(activation_sequence);
    let mut capture_attempt = UmpCaptureAttemptGuard { completed: false };
    let result = YOUTUBE_STREAM_BRIDGE.get_or_init(|| Mutex::new(None));
    *result
        .lock()
        .map_err(|_| "bridge_state_unavailable".to_string())? = None;
    let bridge = ensure_youtube_stream_bridge(&app).await?;
    let mut bridge_cleanup = UmpBridgeCleanupGuard {
        bridge: bridge.clone(),
        token: std::sync::Arc::new(Mutex::new(None)),
        app: app.clone(),
    };
    let (capture_receiver, capture_token) =
        match register_ump_response_capture(&bridge, activation_sequence, false) {
            Ok(capture) => capture,
            Err(_) => {
                ump_capture_failure("capture_setup_failed");
                capture_attempt.completed = true;
                write_native_log(&app, "YouTube UMP response capture registration failed");
                return Err("capture_setup_failed".to_string());
            }
        };
    bridge_cleanup.token = capture_token.clone();
    mute_webview(&bridge).map_err(|error| {
        write_native_log(
            &app,
            &format!("YouTube stream bridge native mute failed: {error}"),
        );
        error
    })?;
    let url = format!("https://music.youtube.com/watch?v={video_id}")
        .parse()
        .map_err(|error: url::ParseError| error.to_string())?;
    bridge.navigate(url).map_err(|error| error.to_string())?;
    let mut resolved_url = None;
    let mut captured_response = None;
    let mut capture_failure = None;
    let mut discarded_captures = 0_u32;
    for _ in 0..100 {
        if resolved_url.is_none() {
            resolved_url = result.lock().map_err(|error| error.to_string())?.take();
        }
        if captured_response.is_none() {
            match capture_receiver.try_recv() {
                Ok(response) => {
                    if is_successful_capture(&response) {
                        captured_response = Some(response);
                    } else {
                        if let Ok(capture) = &response {
                            capture_failure = capture.capture_error;
                        }
                        if discarded_captures == 0 {
                            write_native_log(
                                &app,
                                &format!(
                                    "YouTube UMP capture discarded; waiting for a response with a readable body: {}",
                                    capture_failure.unwrap_or("capture_failed")
                                ),
                            );
                        }
                        discarded_captures = discarded_captures.saturating_add(1);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    captured_response = Some(Err("capture_channel_disconnected"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if captured_response.is_some() && (first_response_only || resolved_url.is_some()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    if captured_response.is_none() {
        if let Some(reason) = capture_failure {
            captured_response = Some(Err(reason));
        }
    }

    let captured_stream_url = captured_response
        .as_ref()
        .and_then(|capture| capture.as_ref().ok())
        .filter(|response| is_captured_audio_ump_response(response))
        .map(|response| response.request_uri.clone());
    let captured_for_audio = captured_response
        .as_ref()
        .and_then(|capture| capture.as_ref().ok())
        .filter(|response| is_captured_audio_ump_response(response))
        .cloned();
    let used_capture_url = captured_stream_url.is_some();

    match captured_response.as_ref() {
        Some(Ok(response)) => {
            let same_stream_request = resolved_url
                .as_deref()
                .is_some_and(|resolved| resolved == response.request_uri);
            if let Some(reason) = response.capture_error {
                let outcome = match reason {
                    "get_content_failed" => "get_content_failed",
                    "content_stream_unavailable" => "content_stream_unavailable",
                    "response_too_large" => "response_too_large",
                    "content_stream_read_failed" => "content_stream_read_failed",
                    "get_content_start_failed" => "get_content_start_failed",
                    _ => "capture_failed",
                };
                let mut snapshot = ump_capture_snapshot();
                ump_capture_response_observed(
                    &mut snapshot,
                    response.status,
                    &response.content_type,
                    &response.request_uri,
                    response.request_headers.clone(),
                    response.request_body.clone(),
                );
                snapshot.activation_sequence = Some(activation_sequence);
                snapshot.same_stream_request = Some(same_stream_request);
                ump_capture_mark_failure(&mut snapshot, outcome);
                update_ump_capture(snapshot);
                write_native_log(
                    &app,
                    &format!("YouTube UMP response metadata captured; body unavailable: {outcome}"),
                );
            } else {
                let (parts, media_bytes) = summarize_ump_capture(&response.body);
                let rust_parse = audio::parse_captured_ump_for_diagnostics(&response.body);
                update_ump_capture(UmpCaptureSnapshot {
                    scope: "webview_capture_only_not_audio_engine_input_proof",
                    stage: "captured",
                    outcome: "success",
                    activation_sequence: Some(activation_sequence),
                    http_status: u16::try_from(response.status).ok(),
                    content_type: response
                        .content_type
                        .split(';')
                        .next()
                        .filter(|value| value.trim().eq_ignore_ascii_case("application/vnd.yt-ump"))
                        .map(|_| "application/vnd.yt-ump"),
                    same_stream_request: Some(same_stream_request),
                    used_capture_as_playback_url: None,
                    request_context: url::Url::parse(&response.request_uri)
                        .ok()
                        .map(|parsed| audio::audio_request_context(&parsed, "POST")),
                    request_headers: response.request_headers.clone(),
                    request_body: response.request_body.clone(),
                    rust_parser_outcome: Some(rust_parse.outcome.clone()),
                    rust_extracted_media_bytes: Some(rust_parse.extracted_media_bytes),
                    body_bytes: response.body.len() as u64,
                    media_bytes,
                    part_kinds: parts.clone(),
                });
                write_native_log(
                    &app,
                    &format!(
                        "YouTube UMP response captured: status={} content_type={} body_bytes={} part_kinds={:?} webview_media_bytes={} rust_parser={} rust_media_bytes={} cap_bytes={}",
                        response.status,
                        if response.content_type.split(';').next().is_some_and(|value| value.trim().eq_ignore_ascii_case("application/vnd.yt-ump")) { "application/vnd.yt-ump" } else { "other" },
                        response.body.len(),
                        parts,
                        media_bytes,
                        rust_parse.outcome,
                        rust_parse.extracted_media_bytes,
                        1024 * 1024
                    ),
                );
                write_native_log(
                    &app,
                    &format!(
                        "YouTube UMP segment trace: segments={:?} formats={:?}",
                        rust_parse.segments, rust_parse.formats
                    ),
                );
            }
        }
        Some(Err(reason)) => {
            let outcome = match *reason {
                "get_content_failed" => "get_content_failed",
                "content_stream_unavailable" => "content_stream_unavailable",
                "response_too_large" => "response_too_large",
                "content_stream_read_failed" => "content_stream_read_failed",
                "get_content_start_failed" => "get_content_start_failed",
                "capture_channel_disconnected" => "capture_channel_disconnected",
                _ => "capture_failed",
            };
            ump_capture_failure(outcome);
            write_native_log(&app, "YouTube UMP response capture failed");
        }
        None => {
            ump_capture_failure("timeout");
            write_native_log(&app, "YouTube UMP response capture timed out");
        }
    }
    ump_capture_record_playback_url_selection(used_capture_url);
    capture_attempt.completed = true;

    if let Some(url) = captured_stream_url.or(resolved_url) {
        write_native_log(
            &app,
            "YouTube Music broker resolved a googlevideo audio stream",
        );
        return Ok((
            serde_json::json!({ "url": url, "mimeType": "audio/mp4" }),
            captured_for_audio,
        ));
    }
    if first_response_only && captured_response.is_some() {
        return Ok((
            serde_json::json!({ "scope": "first_response_only" }),
            captured_response
                .as_ref()
                .and_then(|capture| capture.as_ref().ok())
                .cloned(),
        ));
    }

    write_native_log(
        &app,
        "YouTube stream bridge closed after resolution timeout",
    );
    Err("Tempo limite ao resolver o stream de Ã¡udio do YouTube Music.".to_string())
}

#[cfg(windows)]
#[tauri::command]
async fn resolve_youtube_audio_bridge(
    app: tauri::AppHandle,
    video_id: String,
    activation_sequence: u64,
) -> Result<serde_json::Value, String> {
    resolve_youtube_audio_bridge_inner(app, video_id, activation_sequence, false)
        .await
        .map(|(result, _)| result)
}

#[cfg(windows)]
#[tauri::command]
async fn play_youtube_audio_ump_first_response(
    app: tauri::AppHandle,
    video_id: String,
    activation: audio::PlayerActivation,
) -> Result<serde_json::Value, String> {
    if activation.track.source != "youtube"
        || activation.track.youtube_id.as_deref() != Some(video_id.as_str())
    {
        return Err("activation_track_mismatch".to_string());
    }

    let activation_sequence = activation.activation_sequence;
    let (_, captured) =
        resolve_youtube_audio_bridge_inner(app.clone(), video_id, activation_sequence, true)
            .await?;
    let Some(response) = captured else {
        ump_capture_failure("no_accepted_response");
        return Err("no_accepted_ump_response".to_string());
    };
    if response.capture_error.is_some() {
        ump_capture_failure("body_capture_failed");
        return Err("ump_body_capture_failed".to_string());
    }
    if !is_captured_audio_ump_response(&response) {
        ump_capture_failure("response_validation_failed");
        return Err("ump_response_validation_failed".to_string());
    }
    if response.body.is_empty() || response.body.len() > 1024 * 1024 {
        ump_capture_failure(if response.body.is_empty() {
            "empty_body"
        } else {
            "response_too_large"
        });
        return Err("ump_response_body_invalid".to_string());
    }

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    if sender.try_send(Ok(response.body)).is_err() {
        ump_capture_failure("audio_handoff_failed");
        return Err("ump_audio_handoff_failed".to_string());
    }
    drop(sender);
    let startup = tauri::async_runtime::spawn_blocking(move || {
        audio::AudioEngine::play_ump_receiver(receiver, activation)
    })
    .await
    .map_err(|_| {
        ump_capture_failure("audio_start_failed");
        "audio_start_task_failed".to_string()
    })?;
    if let Err(error) = startup {
        ump_capture_failure("audio_start_failed");
        return Err(error);
    }

    let mut snapshot = ump_capture_snapshot();
    snapshot.scope = "first_response_only_audio_engine_startup";
    snapshot.stage = "audio_startup";
    snapshot.outcome = "success";
    update_ump_capture(snapshot);
    write_native_log(
        &app,
        "YouTube UMP playback scope=first_response_only; no continuation or seek is provided",
    );
    emit_player_event(&app, "trackReplaced");
    Ok(serde_json::json!({
        "scope": "first_response_only",
        "audioEngineStarted": true,
        "continuation": false,
        "seek": false
    }))
}

#[cfg(windows)]
struct UmpBridgeCleanupGuard {
    bridge: tauri::WebviewWindow,
    token: std::sync::Arc<Mutex<Option<i64>>>,
    app: tauri::AppHandle,
}

#[cfg(windows)]
impl Drop for UmpBridgeCleanupGuard {
    fn drop(&mut self) {
        if unregister_ump_response_capture(&self.bridge, &self.token).is_err() {
            write_native_log(
                &self.app,
                "YouTube UMP response capture event cleanup failed",
            );
        }
        if self.bridge.close().is_err() {
            write_native_log(&self.app, "YouTube stream bridge close failed");
        }
    }
}

#[cfg(windows)]
struct ActiveUmpRelay {
    activation_sequence: u64,
    cancel: std::sync::Arc<AtomicBool>,
    done: std::sync::mpsc::Receiver<()>,
}

#[cfg(windows)]
fn ump_relay_slot() -> &'static Mutex<Option<ActiveUmpRelay>> {
    static SLOT: std::sync::OnceLock<Mutex<Option<ActiveUmpRelay>>> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[cfg(windows)]
fn stop_ump_relay_session(app: &tauri::AppHandle, session: ActiveUmpRelay, reason: &str) {
    session.cancel.store(true, Ordering::Release);
    match session.done.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(()) => write_native_log(
            app,
            &format!("YouTube UMP relay session cleaned up: {reason}"),
        ),
        Err(_) => write_native_log(
            app,
            &format!("YouTube UMP relay session cleanup timed out: {reason}"),
        ),
    }
}

#[cfg(windows)]
fn cancel_active_ump_relay(app: &tauri::AppHandle) {
    let session = ump_relay_slot()
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some(session) = session {
        stop_ump_relay_session(app, session, "replaced");
    }
}

#[cfg(windows)]
fn cancel_ump_relay_for_sequence(app: &tauri::AppHandle, activation_sequence: u64) {
    let session = {
        let Ok(mut slot) = ump_relay_slot().lock() else {
            return;
        };
        match slot.as_ref() {
            Some(active) if active.activation_sequence == activation_sequence => slot.take(),
            _ => None,
        }
    };
    if let Some(session) = session {
        stop_ump_relay_session(app, session, "startup_failed");
    }
}

#[cfg(windows)]
fn start_ump_relay(
    app: tauri::AppHandle,
    bridge: tauri::WebviewWindow,
    cleanup: UmpBridgeCleanupGuard,
    capture_receiver: std::sync::mpsc::Receiver<Result<CapturedUmpResponse, &'static str>>,
    body_sender: std::sync::mpsc::SyncSender<Result<Vec<u8>, String>>,
    activation_sequence: u64,
) -> ActiveUmpRelay {
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let thread_cancel = cancel.clone();
    let (done_sender, done) = std::sync::mpsc::channel();
    let started_at = std::time::Instant::now();
    std::thread::spawn(move || {
        let mut forwarded_bodies = 0_u64;
        let mut forwarded_bytes = 0_u64;
        let mut discarded_bodies = 0_u64;
        let mut last_paused: Option<bool> = None;
        loop {
            if thread_cancel.load(Ordering::Acquire)
                || UMP_RELAY_PAGE_ENDED.load(Ordering::Acquire)
                || UMP_RELAY_PAGE_ERROR.load(Ordering::Acquire)
            {
                break;
            }
            if started_at.elapsed() > std::time::Duration::from_secs(5)
                && forwarded_bodies > 0
                && !audio::AudioEngine::is_active()
            {
                write_native_log(&app, "YouTube UMP relay stopped: audio engine inactive");
                break;
            }
            let paused = audio::AudioEngine::is_paused();
            if last_paused != Some(paused) {
                last_paused = Some(paused);
                let script = if paused {
                    "document.querySelector('video')?.pause?.();"
                } else {
                    "(() => { const v = document.querySelector('video'); if (v) { v.muted = true; v.volume = 0; const p = v.play?.(); if (p && p.catch) p.catch(() => {}); } })();"
                };
                let _ = bridge.eval(script);
            }
            match capture_receiver.recv_timeout(std::time::Duration::from_millis(150)) {
                Ok(Ok(response)) => {
                    if response.capture_error.is_some() || response.body.is_empty() {
                        discarded_bodies = discarded_bodies.saturating_add(1);
                        continue;
                    }
                    if response.body.len() > 1024 * 1024 {
                        discarded_bodies = discarded_bodies.saturating_add(1);
                        continue;
                    }
                    let body_len = response.body.len() as u64;
                    if send_with_timeout(
                        &body_sender,
                        Ok(response.body),
                        std::time::Duration::from_secs(5),
                    )
                    .is_err()
                    {
                        write_native_log(
                            &app,
                            "YouTube UMP relay stopped: audio engine channel closed",
                        );
                        break;
                    }
                    forwarded_bodies = forwarded_bodies.saturating_add(1);
                    forwarded_bytes = forwarded_bytes.saturating_add(body_len);
                }
                Ok(Err(reason)) => {
                    write_native_log(
                        &app,
                        &format!("YouTube UMP relay capture channel failed: {reason}"),
                    );
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let _ = bridge.eval("document.querySelector('video')?.pause?.();");
        drop(body_sender);
        drop(cleanup);
        write_native_log(
            &app,
            &format!(
                "YouTube UMP relay ended: activation={activation_sequence} bodies={forwarded_bodies} bytes={forwarded_bytes} discarded={discarded_bodies} page_ended={}",
                UMP_RELAY_PAGE_ENDED.load(Ordering::Acquire)
            ),
        );
        let _ = done_sender.send(());
    });
    ActiveUmpRelay {
        activation_sequence,
        cancel,
        done,
    }
}

#[cfg(windows)]
#[tauri::command]
async fn play_youtube_audio_ump_stream(
    app: tauri::AppHandle,
    video_id: String,
    activation: audio::PlayerActivation,
) -> Result<serde_json::Value, String> {
    if activation.track.source != "youtube"
        || activation.track.youtube_id.as_deref() != Some(video_id.as_str())
    {
        return Err("activation_track_mismatch".to_string());
    }
    let activation_sequence = activation.activation_sequence;

    cancel_active_ump_relay(&app);
    // Let a just-closed bridge window finish tearing down before reusing the label.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let bridge = ensure_youtube_stream_bridge(&app).await?;
    let mut cleanup = UmpBridgeCleanupGuard {
        bridge: bridge.clone(),
        token: std::sync::Arc::new(Mutex::new(None)),
        app: app.clone(),
    };
    let (capture_receiver, capture_token) =
        register_ump_response_capture(&bridge, activation_sequence, true).map_err(|error| {
            write_native_log(&app, "YouTube UMP relay capture registration failed");
            error
        })?;
    cleanup.token = capture_token;
    mute_webview(&bridge).map_err(|error| {
        write_native_log(
            &app,
            &format!("YouTube stream bridge native mute failed: {error}"),
        );
        error
    })?;

    UMP_RELAY_PAGE_ENDED.store(false, Ordering::Release);
    UMP_RELAY_PAGE_ERROR.store(false, Ordering::Release);
    write_native_log(
        &app,
        &format!("YouTube UMP relay starting: activation={activation_sequence}"),
    );

    let url = format!("https://music.youtube.com/watch?v={video_id}")
        .parse()
        .map_err(|error: url::ParseError| error.to_string())?;
    bridge.navigate(url).map_err(|error| error.to_string())?;

    let (body_sender, body_receiver) = std::sync::mpsc::sync_channel(8);
    let session = start_ump_relay(
        app.clone(),
        bridge,
        cleanup,
        capture_receiver,
        body_sender,
        activation_sequence,
    );
    if let Ok(mut slot) = ump_relay_slot().lock() {
        *slot = Some(session);
    }

    let startup = tauri::async_runtime::spawn_blocking(move || {
        audio::AudioEngine::play_ump_receiver(body_receiver, activation)
    })
    .await
    .map_err(|_| {
        cancel_ump_relay_for_sequence(&app, activation_sequence);
        "audio_start_task_failed".to_string()
    })?;
    if let Err(error) = startup {
        cancel_ump_relay_for_sequence(&app, activation_sequence);
        return Err(error);
    }

    let mut snapshot = ump_capture_snapshot();
    snapshot.scope = "continuous_sabr_relay_audio_engine";
    snapshot.stage = "audio_startup";
    snapshot.outcome = "success";
    update_ump_capture(snapshot);
    write_native_log(
        &app,
        "YouTube UMP playback scope=continuous_sabr_relay; WebView-managed SABR transport; seek not provided",
    );
    eprintln!("VibeDeck playback path=continuous_sabr_relay (WebView broker)");
    emit_player_event(&app, "trackReplaced");
    Ok(serde_json::json!({
        "scope": "continuous_sabr_relay",
        "audioEngineStarted": true,
        "continuation": true,
        "seek": false
    }))
}

/// Native byte-range playback (no WebView broker, no `pot`). Only compiled with the
/// `native-range-playback` feature; the frontend asks `is_native_range_available` first and
/// falls back to the relay when this fails.
#[cfg(all(windows, feature = "native-range-playback"))]
#[tauri::command]
async fn play_youtube_audio_native_range(
    app: tauri::AppHandle,
    video_id: String,
    activation: audio::PlayerActivation,
) -> Result<serde_json::Value, String> {
    if activation.track.source != "youtube"
        || activation.track.youtube_id.as_deref() != Some(video_id.as_str())
    {
        return Err("activation_track_mismatch".to_string());
    }
    cancel_active_ump_relay(&app);
    let result = native_range_session::play(&app, &video_id, activation).await?;
    write_native_log(
        &app,
        "YouTube playback scope=native_byte_range; native requests, seek supported",
    );
    eprintln!("VibeDeck playback path=native_byte_range (no relay, no WebView broker)");
    emit_player_event(&app, "trackReplaced");
    Ok(result)
}

#[cfg(windows)]
#[tauri::command]
fn is_native_range_available() -> bool {
    cfg!(feature = "native-range-playback")
}

#[cfg(windows)]
fn collect_music_results(value: &serde_json::Value, results: &mut Vec<serde_json::Value>) {
    if results.len() >= 25 {
        return;
    }
    if let Some(renderer) = value
        .get("musicResponsiveListItemRenderer")
        .or_else(|| value.get("videoRenderer"))
    {
        if let Some(video_id) = find_string_field(renderer, "videoId") {
            if !results.iter().any(|item| {
                item.get("youtubeId").and_then(|value| value.as_str()) == Some(video_id)
            }) {
                let title = renderer.pointer("/title/runs/0/text").and_then(|value| value.as_str()).or_else(|| renderer.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text").and_then(|value| value.as_str())).unwrap_or("YouTube Music");
                let metadata = renderer
                    .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs")
                    .and_then(|value| value.as_array())
                    .map(|runs| {
                        runs.iter()
                            .filter_map(|run| run.get("text").and_then(|value| value.as_str()))
                            .filter(|text| {
                                *text != " â€¢ "
                                    && *text != "MÃºsica"
                                    && *text != "VÃ­deo"
                                    && *text != "Ãlbum"
                                    && *text != "Playlist"
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let artist = renderer
                    .pointer("/ownerText/runs/0/text")
                    .and_then(|value| value.as_str())
                    .or_else(|| metadata.iter().copied().find(|text| !is_duration(text)))
                    .unwrap_or("YouTube");
                let album = metadata
                    .iter()
                    .copied()
                    .filter(|text| !is_duration(text))
                    .nth(1)
                    .unwrap_or("YouTube Music");
                let duration = renderer.pointer("/lengthText/simpleText").and_then(|value| value.as_str()).or_else(|| renderer.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/simpleText").and_then(|value| value.as_str())).or_else(|| renderer.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text").and_then(|value| value.as_str())).or_else(|| metadata.iter().copied().find(|text| is_duration(text))).unwrap_or("--:--");
                let cover_url = renderer
                    .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
                    .and_then(|value| value.as_array())
                    .and_then(|thumbnails| thumbnails.last())
                    .and_then(|thumbnail| thumbnail.get("url"))
                    .and_then(|value| value.as_str());
                results.push(serde_json::json!({ "id": format!("youtube-{video_id}"), "youtubeId": video_id, "title": title, "artist": artist, "album": album, "duration": duration, "coverUrl": cover_url, "source": "youtube", "accent": "#ff5f55" }));
            }
        }
    }
    match value {
        serde_json::Value::Object(map) => {
            for child in map.values() {
                collect_music_results(child, results);
            }
        }
        serde_json::Value::Array(items) => {
            for child in items {
                collect_music_results(child, results);
            }
        }
        _ => {}
    }
}

#[cfg(windows)]
fn find_string_field<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    if let Some(result) = value.get(field).and_then(|item| item.as_str()) {
        return Some(result);
    }
    match value {
        serde_json::Value::Object(map) => {
            map.values().find_map(|item| find_string_field(item, field))
        }
        serde_json::Value::Array(items) => {
            items.iter().find_map(|item| find_string_field(item, field))
        }
        _ => None,
    }
}

#[cfg(windows)]
fn is_duration(value: &str) -> bool {
    let parts = value.split(':').collect::<Vec<_>>();
    (parts.len() == 2 || parts.len() == 3)
        && parts.iter().all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        })
}

#[cfg(windows)]
fn emit_player_event(app: &tauri::AppHandle, kind: &str) {
    let _ = app.emit(
        "player-state",
        serde_json::json!({ "kind": kind, "snapshot": audio::AudioEngine::snapshot() }),
    );
}

#[cfg(windows)]
#[tauri::command]
fn play_audio_url(
    app: tauri::AppHandle,
    url: String,
    activation: audio::PlayerActivation,
) -> Result<(), String> {
    eprintln!("VibeDeck player activation start: command=play_audio_url intent={} generation={} index={} track={} revision={}", activation.activation_sequence, activation.queue_generation, activation.queue_index, activation.track.id, audio::AudioEngine::snapshot().revision);
    audio::AudioEngine::play_url(url, activation)?;
    emit_player_event(&app, "trackReplaced");
    let snapshot = audio::AudioEngine::snapshot();
    eprintln!("VibeDeck player activation complete: command=play_audio_url intent={} generation={} index={} track={} revision={}", snapshot.activation_sequence, snapshot.queue_generation, snapshot.queue_index, snapshot.track.as_ref().map(|track| track.id.as_str()).unwrap_or("none"), snapshot.revision);
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
fn play_local_audio(
    app: tauri::AppHandle,
    request: local_library::ResolveLocalTrackRequest,
    activation: audio::PlayerActivation,
) -> Result<audio::PlayerSnapshot, local_library::LocalPlaybackBridgeError> {
    let correlation_id = request.correlation_id.clone();
    eprintln!("VibeDeck player activation start: command=play_local_audio intent={} generation={} index={} track={} correlation={}", activation.activation_sequence, activation.queue_generation, activation.queue_index, activation.track.id, correlation_id);
    let path = local_library::resolve_local_track(&app, request).map_err(|error| {
        local_library::LocalPlaybackBridgeError::from_error(error, correlation_id.clone())
    })?;
    audio::AudioEngine::validate_file(&path)
        .map_err(|error| match error {
            audio::FileValidationError::Io(detail) => local_library::LocalPlaybackError::Io(detail),
            audio::FileValidationError::UnsupportedContainer => {
                local_library::LocalPlaybackError::UnsupportedContainer
            }
            audio::FileValidationError::InvalidContainer(detail) => {
                local_library::LocalPlaybackError::InvalidContainer(detail)
            }
            audio::FileValidationError::DecoderFailure(detail) => {
                local_library::LocalPlaybackError::DecoderFailure(detail)
            }
        })
        .map_err(|error| {
            local_library::LocalPlaybackBridgeError::from_error(error, correlation_id.clone())
        })?;
    audio::AudioEngine::validate_output_device().map_err(|error| {
        local_library::LocalPlaybackBridgeError::from_error(
            local_library::LocalPlaybackError::NoOutputDevice(error),
            correlation_id.clone(),
        )
    })?;
    audio::AudioEngine::play_file(path, activation).map_err(|error| {
        let playback_error = error
            .strip_prefix("outputStart: ")
            .map(|detail| local_library::LocalPlaybackError::NoOutputDevice(detail.to_string()))
            .unwrap_or(local_library::LocalPlaybackError::Io(error));
        local_library::LocalPlaybackBridgeError::from_error(playback_error, correlation_id.clone())
    })?;
    emit_player_event(&app, "trackReplaced");
    let snapshot = audio::AudioEngine::snapshot();
    eprintln!("VibeDeck player activation complete: command=play_local_audio intent={} generation={} index={} track={} revision={} correlation={}", snapshot.activation_sequence, snapshot.queue_generation, snapshot.queue_index, snapshot.track.as_ref().map(|track| track.id.as_str()).unwrap_or("none"), snapshot.revision, correlation_id);
    Ok(snapshot)
}

#[cfg(windows)]
#[tauri::command]
fn stop_audio(app: tauri::AppHandle) {
    #[cfg(feature = "native-range-playback")]
    native_range_session::cancel();
    audio::AudioEngine::stop();
    emit_player_event(&app, "stateChanged");
}

#[cfg(windows)]
#[tauri::command]
fn set_audio_paused(app: tauri::AppHandle, paused: bool) {
    audio::AudioEngine::set_paused(paused);
    emit_player_event(&app, "stateChanged");
}

#[cfg(windows)]
#[tauri::command]
fn set_audio_repeat_mode(
    app: tauri::AppHandle,
    request: audio::RepeatModeRequest,
) -> Result<audio::PlayerSnapshot, String> {
    audio::AudioEngine::set_repeat_mode(request)?;
    emit_player_event(&app, "stateChanged");
    Ok(audio::AudioEngine::snapshot())
}

#[cfg(windows)]
#[tauri::command]
fn set_audio_volume(app: tauri::AppHandle, request: audio::VolumeRequest) -> audio::PlayerSnapshot {
    audio::AudioEngine::set_volume(request);
    emit_player_event(&app, "stateChanged");
    audio::AudioEngine::snapshot()
}

#[cfg(windows)]
#[tauri::command]
async fn seek_audio(
    app: tauri::AppHandle,
    request: audio::SeekRequest,
) -> Result<audio::PlayerSnapshot, String> {
    #[cfg(feature = "native-range-playback")]
    if native_range_session::seek(&request).await? {
        let snapshot = audio::AudioEngine::snapshot();
        emit_player_event(&app, "stateChanged");
        return Ok(snapshot);
    }
    audio::AudioEngine::seek(request)?;
    let snapshot = audio::AudioEngine::snapshot();
    emit_player_event(&app, "stateChanged");
    Ok(snapshot)
}

#[cfg(windows)]
#[tauri::command]
fn get_audio_position() -> f64 {
    audio::AudioEngine::position()
}

#[cfg(windows)]
#[tauri::command]
fn get_audio_duration() -> f64 {
    audio::AudioEngine::duration()
}

#[cfg(windows)]
#[tauri::command]
fn get_audio_levels() -> Vec<f32> {
    audio::AudioEngine::levels()
}

#[cfg(windows)]
#[tauri::command]
fn is_audio_active() -> bool {
    audio::AudioEngine::is_active()
}

#[cfg(windows)]
#[tauri::command]
fn is_audio_paused() -> bool {
    audio::AudioEngine::is_paused()
}

#[cfg(windows)]
#[tauri::command]
fn get_player_snapshot() -> audio::PlayerSnapshot {
    audio::AudioEngine::snapshot()
}

#[cfg(windows)]
#[tauri::command]
fn get_audio_playback_diagnostic() -> serde_json::Value {
    let snapshot = audio::AudioEngine::diagnostics_snapshot();
    serde_json::json!({
        "activationSequence": snapshot.activation_sequence,
        "playerActivationSequence": snapshot.player_activation_sequence,
        "stage": snapshot.stage,
        "outcome": snapshot.outcome,
        "httpStatus": snapshot.http_status,
        "responseBytes": snapshot.response_bytes,
        "mediaBytes": snapshot.media_bytes,
        "umpPartKinds": snapshot.ump_part_kinds,
        "umpSegments": snapshot.ump_segments,
        "umpFormats": snapshot.ump_formats,
        "decoderStarted": snapshot.decoder_started,
        "outputStarted": snapshot.output_started,
        "errorCode": snapshot.error_code,
        "probeErrorKind": snapshot.probe_error_kind,
        "updatedAtUnixMs": snapshot.updated_at_unix_ms,
    })
}

#[cfg(all(test, windows))]
mod youtube_healthcheck_tests {
    use super::*;

    #[test]
    fn audio_playback_diagnostic_exposes_only_allowlisted_fields() {
        let diagnostic = get_audio_playback_diagnostic();
        let object = diagnostic.as_object().expect("diagnostic object");
        assert!(object.contains_key("playerActivationSequence"));
        assert!(object.contains_key("errorCode"));
        assert!(object.contains_key("probeErrorKind"));
        assert!(object.contains_key("umpSegments"));
        assert!(object.contains_key("umpFormats"));
        assert!(object.contains_key("decoderStarted"));
        assert!(object.contains_key("outputStarted"));
        assert!(!object.contains_key("requestContext"));
        assert!(!object.contains_key("requestHeaders"));
        assert!(!object.contains_key("requestBody"));
        assert!(!object.contains_key("url"));
    }

    #[test]
    fn ump_prefilter_diagnostics_are_sanitized_and_bounded() {
        let parsed = url::Url::parse(
            "https://private-subdomain.googlevideo.com/videoplayback?pot=do-not-log&range=0-64&private%20key=secret-value",
        )
        .unwrap();
        let names = sanitize_ump_query_parameter_names(&parsed);
        let serialized_names = serde_json::to_string(&names).unwrap();
        assert!(names.contains(&"pot".to_string()));
        assert!(names.contains(&"range".to_string()));
        assert!(names.contains(&"privatekey".to_string()));
        assert!(!serialized_names.contains("do-not-log"));
        assert!(!serialized_names.contains("0-64"));
        assert!(!serialized_names.contains("secret-value"));
        assert!(!serialized_names.contains("private-subdomain"));
        assert!(names.len() <= 24);
        assert!(names.iter().all(|name| name.len() <= 32));

        assert_eq!(ump_response_content_type_category(None), "absent");
        assert_eq!(
            ump_response_content_type_category(Some("application/vnd.yt-ump; private=value",)),
            "ump"
        );
        assert_eq!(
            ump_response_content_type_category(Some("application/octet-stream")),
            "other"
        );
        assert_eq!(
            ump_response_rejection_reason(true, true, true, Some(200), "ump"),
            "accepted"
        );
        assert_eq!(
            ump_response_rejection_reason(true, true, true, Some(403), "ump"),
            "non_success_status"
        );

        let slots = AtomicUsize::new(0);
        let logged = (0..32)
            .filter(|_| reserve_ump_diagnostic_slot(&slots))
            .count();
        assert_eq!(logged, 16);
        assert_eq!(slots.load(Ordering::Acquire), 16);
    }

    #[test]
    fn skipped_checks_serialize_without_sensitive_fields() {
        let audio_snapshot = serde_json::to_value(audio::AudioEngine::diagnostics_snapshot())
            .unwrap_or(serde_json::Value::Null);
        let report = YouTubeHealthCheckReport {
            checks: vec![skipped_health_check("search")],
            player: YouTubeHealthCheckPlayer {
                status: "idle",
                source: "none",
                position_seconds: Some(0.0),
                duration_seconds: Some(0.0),
                queue_length: 0,
                queue_index: -1,
                queue_generation: 0,
                activation_sequence: 0,
                native_mode: false,
            },
            audio: audio_snapshot.clone(),
            ump_capture: UmpCaptureSnapshot {
                scope: "webview_capture_only_not_audio_engine_input_proof",
                stage: "waiting",
                outcome: "not_attempted",
                activation_sequence: None,
                http_status: None,
                content_type: None,
                same_stream_request: None,
                used_capture_as_playback_url: None,
                request_context: None,
                request_headers: None,
                request_body: None,
                rust_parser_outcome: None,
                rust_extracted_media_bytes: None,
                body_bytes: 0,
                media_bytes: 0,
                part_kinds: Vec::new(),
            },
        };
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "checks": [{ "operation": "search", "status": "skipped" }],
                "player": {
                    "status": "idle",
                    "source": "none",
                    "positionSeconds": 0.0,
                    "durationSeconds": 0.0,
                    "queueLength": 0,
                    "queueIndex": -1,
                    "queueGeneration": 0,
                    "activationSequence": 0,
                    "nativeMode": false
                },
                "audio": audio_snapshot,
                "umpCapture": {
                    "scope": "webview_capture_only_not_audio_engine_input_proof",
                    "stage": "waiting",
                    "outcome": "not_attempted",
                    "bodyBytes": 0,
                    "mediaBytes": 0,
                    "partKinds": []
                }
            })
        );
        let serialized = value.to_string().to_ascii_lowercase();
        for forbidden in [
            "cookie",
            "signature",
            "videoid",
            "title",
            "query",
            "range",
            "url",
            "rawbody",
            "rawerror",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "unexpected report field/content: {forbidden}"
            );
        }
        assert_eq!(
            value["umpCapture"]["scope"],
            "webview_capture_only_not_audio_engine_input_proof"
        );
    }

    #[test]
    fn ump_capture_summary_is_numeric_bounded_and_payload_free() {
        let body = [0x15, 0x03, 0xaa, 0xbb, 0xcc, 0x02, 0x01, 0xff];
        let (parts, media_bytes) = summarize_ump_capture(&body);
        assert_eq!(
            parts,
            vec![
                UmpPartKindCount { kind: 2, count: 1 },
                UmpPartKindCount { kind: 21, count: 1 }
            ]
        );
        assert_eq!(media_bytes, 2);
        assert!(parts.len() <= 32);
        let serialized = serde_json::to_string(&parts).unwrap();
        assert!(!serialized.contains("aa"));
    }

    #[test]
    fn webview_capture_failure_preserves_allowlisted_response_metadata() {
        let mut snapshot = UmpCaptureSnapshot {
            scope: "webview_capture_only_not_audio_engine_input_proof",
            stage: "waiting",
            outcome: "in_progress",
            activation_sequence: Some(1),
            http_status: None,
            content_type: None,
            same_stream_request: None,
            used_capture_as_playback_url: None,
            request_context: None,
            request_headers: None,
            request_body: None,
            rust_parser_outcome: None,
            rust_extracted_media_bytes: None,
            body_bytes: 0,
            media_bytes: 0,
            part_kinds: Vec::new(),
        };

        ump_capture_response_observed(
            &mut snapshot,
            200,
            "application/vnd.yt-ump; private=not-retained",
            "https://cdn.googlevideo.com/videoplayback?ump=1&itag=140&range=0-10&clen=999&pot=secret&cpn=private",
            None,
            None,
        );
        ump_capture_mark_failure(&mut snapshot, "content_stream_unavailable");

        assert_eq!(snapshot.stage, "capture");
        assert_eq!(snapshot.outcome, "content_stream_unavailable");
        assert_eq!(snapshot.http_status, Some(200));
        assert_eq!(snapshot.content_type, Some("application/vnd.yt-ump"));
        assert_eq!(snapshot.same_stream_request, None);
        assert_eq!(snapshot.body_bytes, 0);
        let serialized = serde_json::to_string(&snapshot).unwrap();
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("not-retained"));
    }

    #[test]
    fn captured_ump_acceptance_does_not_require_query_audio_markers() {
        let response = |status, mime: &str| CapturedUmpResponse {
            status,
            request_method: "POST".into(),
            content_type: "application/vnd.yt-ump".into(),
            request_uri: format!("https://cdn.googlevideo.com/videoplayback?itag={mime}"),
            request_headers: None,
            request_body: None,
            body: Vec::new(),
            capture_error: None,
        };
        assert!(is_captured_audio_ump_response(&response(200, "140")));
        assert!(is_captured_audio_ump_response(&response(200, "video")));
        assert!(!is_captured_audio_ump_response(&response(400, "140")));
        let mut failed_capture = response(200, "140");
        failed_capture.capture_error = Some("content_stream_read_failed");
        assert!(!is_captured_audio_ump_response(&failed_capture));
    }

    #[test]
    fn capture_success_requires_a_readable_body() {
        let response = |capture_error: Option<&'static str>| CapturedUmpResponse {
            status: 200,
            request_method: "POST".into(),
            content_type: "application/vnd.yt-ump".into(),
            request_uri: "https://cdn.googlevideo.com/videoplayback?itag=140".into(),
            request_headers: None,
            request_body: None,
            body: Vec::new(),
            capture_error,
        };
        assert!(!is_successful_capture(&Ok(response(Some(
            "content_stream_unavailable"
        )))));
        assert!(!is_successful_capture(&Err("capture_channel_disconnected")));
        assert!(is_successful_capture(&Ok(response(None))));
    }

    #[test]
    fn ump_acceptance_uses_response_method_host_path_status_and_mime_only() {
        let no_marker_url = "https://r.googlevideo.com/videoplayback?itag=140&range=0-64";
        assert!(is_acceptable_ump_response(
            "POST",
            no_marker_url,
            200,
            "application/vnd.yt-ump; charset=binary"
        ));
        assert!(!is_acceptable_ump_response(
            "GET",
            no_marker_url,
            200,
            "application/vnd.yt-ump"
        ));
        assert!(!is_acceptable_ump_response(
            "POST",
            "https://r.googlevideo.com/not-videoplayback?itag=140",
            200,
            "application/vnd.yt-ump"
        ));
        assert!(!is_acceptable_ump_response(
            "POST",
            no_marker_url,
            403,
            "application/vnd.yt-ump"
        ));
        assert!(!is_acceptable_ump_response(
            "POST",
            no_marker_url,
            200,
            "application/octet-stream"
        ));
    }

    #[test]
    fn player_projection_allowlists_state_and_sanitizes_values() {
        assert_eq!(health_check_player_status(false, false, false), "idle");
        assert_eq!(health_check_player_status(true, true, false), "playing");
        assert_eq!(health_check_player_status(true, true, true), "paused");
        assert_eq!(health_check_player_status(true, false, false), "stopped");
        assert_eq!(health_check_player_source(Some("youtube")), "youtube");
        assert_eq!(health_check_player_source(Some("local")), "local");
        assert_eq!(health_check_player_source(Some("private-value")), "other");
        assert_eq!(health_check_player_source(None), "none");
        assert_eq!(finite_nonnegative(3.5), Some(3.5));
        assert_eq!(finite_nonnegative(-1.0), None);
        assert_eq!(finite_nonnegative(f64::NAN), None);

        let value = serde_json::to_value(YouTubeHealthCheckPlayer {
            status: "stopped",
            source: "other",
            position_seconds: None,
            duration_seconds: None,
            queue_length: 1,
            queue_index: 0,
            queue_generation: 2,
            activation_sequence: 3,
            native_mode: true,
        })
        .unwrap();
        assert_eq!(value["source"], "other");
        assert_eq!(value["positionSeconds"], serde_json::Value::Null);
        assert!(value.get("track").is_none());
        assert!(value.get("error").is_none());
        assert!(value.get("playlistId").is_none());
    }

    #[test]
    fn error_codes_are_stable_and_provider_text_is_not_returned() {
        assert_eq!(
            health_check_error_code("YouTube search HTTP 429"),
            "http_429"
        );
        assert_eq!(
            health_check_error_code("private provider response"),
            "request_failed"
        );
    }
}

#[cfg(all(windows, debug_assertions))]
fn app_invoke_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        sabr::bootstrap_probe::debug_youtube_bootstrap_once,
        sabr::bootstrap_structure_probe::debug_youtube_bootstrap_structure_once,
        sabr::bootstrap_structure_probe::debug_youtube_bootstrap_structure_counted,
        #[cfg(feature = "bootstrap-inspection")]
        sabr::bootstrap_script_probe::debug_youtube_bootstrap_script_once,
        #[cfg(feature = "native-playback-trial")]
        sabr::native_trial::debug_native_playback_trial,
        sabr::script_probe::debug_youtube_player_script_once,
        sabr::client_survey::debug_youtube_player_client_survey,
        youtube_browse::probe::debug_youtube_browse_probe,
        #[cfg(feature = "native-playback-trial")]
        sabr::native_media_probe::debug_youtube_native_media_once,
        #[cfg(feature = "native-playback-trial")]
        sabr::native_playback_probe::debug_youtube_native_playback_once,
        sabr::range_probe::debug_youtube_range_probe_once,
        sabr::session_probe::debug_youtube_sabr_session_once,
        sabr::transport_probe::debug_youtube_sabr_transport_once,
        youtube_session_debug::debug_youtube_session,
        get_visualizer_controls_preference,
        set_visualizer_controls_preference,
        get_serpents_preference,
        set_serpents_preference,
        protect_secret,
        unprotect_secret,
        start_oauth_listener,
        take_oauth_callback,
        open_external_url,
        save_secure_record,
        load_secure_record,
        open_youtube_login,
        capture_youtube_session,
        youtube_session_available,
        logout_youtube_music,
        exchange_google_code,
        refresh_google_token,
        resolve_youtube_audio,
        resolve_youtube_audio_bridge,
        play_youtube_audio_ump_first_response,
        play_youtube_audio_ump_stream,
        #[cfg(feature = "native-range-playback")]
        play_youtube_audio_native_range,
        is_native_range_available,
        search_youtube_music,
        youtube_music_healthcheck,
        load_youtube_playlists,
        debug_youtube_playlist_probe_status,
        debug_youtube_sabr_probe,
        load_youtube_playlist_tracks,
        load_youtube_playlist_more_tracks,
        get_youtube_playlist_debug_state,
        get_app_logs,
        clear_app_logs,
        write_app_log,
        get_app_webviews,
        set_app_webview_visible,
        export_app_logs,
        play_audio_url,
        play_local_audio,
        stop_audio,
        set_audio_paused,
        set_audio_repeat_mode,
        set_audio_volume,
        seek_audio,
        get_audio_position,
        get_audio_duration,
        get_audio_levels,
        is_audio_active,
        is_audio_paused,
        get_player_snapshot,
        get_audio_playback_diagnostic,
        local_library::load_local_library,
        local_library::import_local_paths,
        local_library::cancel_local_import,
        local_library::remove_local_track,
        playlist_catalog::load_playlist_catalog,
        playlist_catalog::save_youtube_playlist_page
    ]
}

#[cfg(not(all(windows, debug_assertions)))]
fn app_invoke_handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool {
    tauri::generate_handler![
        get_visualizer_controls_preference,
        set_visualizer_controls_preference,
        get_serpents_preference,
        set_serpents_preference,
        protect_secret,
        unprotect_secret,
        start_oauth_listener,
        take_oauth_callback,
        open_external_url,
        save_secure_record,
        load_secure_record,
        open_youtube_login,
        capture_youtube_session,
        youtube_session_available,
        logout_youtube_music,
        exchange_google_code,
        refresh_google_token,
        resolve_youtube_audio,
        resolve_youtube_audio_bridge,
        play_youtube_audio_ump_first_response,
        play_youtube_audio_ump_stream,
        search_youtube_music,
        youtube_music_healthcheck,
        load_youtube_playlists,
        load_youtube_playlist_tracks,
        load_youtube_playlist_more_tracks,
        get_youtube_playlist_debug_state,
        get_app_logs,
        clear_app_logs,
        write_app_log,
        get_app_webviews,
        set_app_webview_visible,
        export_app_logs,
        play_audio_url,
        play_local_audio,
        stop_audio,
        set_audio_paused,
        set_audio_repeat_mode,
        set_audio_volume,
        seek_audio,
        get_audio_position,
        get_audio_duration,
        get_audio_levels,
        is_audio_active,
        is_audio_paused,
        get_player_snapshot,
        get_audio_playback_diagnostic,
        local_library::load_local_library,
        local_library::import_local_paths,
        local_library::cancel_local_import,
        local_library::remove_local_track,
        playlist_catalog::load_playlist_catalog,
        playlist_catalog::save_youtube_playlist_page
    ]
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    audio::AudioEngine::stop();
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(app_invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running VibeDeck");
}

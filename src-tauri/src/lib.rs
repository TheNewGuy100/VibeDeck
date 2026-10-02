#[cfg(windows)]
use std::collections::{HashMap, HashSet};
#[cfg(windows)]
use std::io::{Read, Write};
#[cfg(windows)]
use std::net::TcpListener;
#[cfg(windows)]
use std::ptr;
#[cfg(windows)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sha1::{Digest, Sha1};

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
static YOUTUBE_LIBRARY_BRIDGE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_LIBRARY_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_LOGIN_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_PLAYLIST_BRIDGE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_PLAYLIST_CHUNKS: OnceLock<Mutex<Vec<Option<String>>>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_PLAYLIST_REQUEST: OnceLock<Mutex<String>> = OnceLock::new();

#[cfg(windows)]
static YOUTUBE_STREAM_BRIDGE: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[cfg(windows)]
static APP_LOGS: OnceLock<Mutex<Vec<AppLogEntry>>> = OnceLock::new();

#[cfg(windows)]
static APP_LOG_ID: AtomicU64 = AtomicU64::new(1);

#[cfg(windows)]
static MALFORMED_PLAYLIST_CHUNK_LOGGED: OnceLock<Mutex<bool>> = OnceLock::new();

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
        "youtube-library-bridge" => ("Broker de biblioteca e playlists", true),
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
    if lower.contains("library bridge") {
        return (
            level,
            Some("youtube-library-bridge"),
            "library",
            "youtube-library-bridge",
        );
    }
    if lower.contains("playlist bridge")
        || lower.contains("playlist tracks")
        || lower.contains("playlist")
    {
        return (
            level,
            Some("youtube-library-bridge"),
            "playlist",
            "youtube-playlist",
        );
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
            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!doctype html><html lang=\"pt-BR\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>VibeDeck conectado</title><style>*{box-sizing:border-box}body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0b0d12;color:#f4f5f7;font-family:Arial,sans-serif}.card{width:min(520px,calc(100% - 32px));padding:42px;border:1px solid #29313b;border-radius:22px;background:linear-gradient(145deg,#151b20,#101319);text-align:center;box-shadow:0 24px 80px #0008}.mark{width:58px;height:58px;margin:0 auto 22px;display:grid;place-items:center;border-radius:17px;color:#0b0d12;background:#b9f36b;font-size:30px;font-weight:700}.eyebrow{margin:0 0 12px;color:#9ca8a1;font-size:11px;font-weight:700;letter-spacing:2px}.title{margin:0;font-size:28px}.copy{margin:14px auto 28px;max-width:360px;color:#929ba1;line-height:1.6}.countdown{color:#b9f36b;font-size:13px}.countdown strong{font-size:18px}.hint{margin-top:22px;color:#68737a;font-size:11px}@media(max-width:480px){.card{padding:30px 22px}.title{font-size:23px}}</style></head><body><main class=\"card\"><div class=\"mark\">V</div><p class=\"eyebrow\">VIBEDECK</p><h1 class=\"title\">Conta conectada</h1><p class=\"copy\">A autorização do Google foi concluída. Você já pode voltar ao aplicativo e sincronizar suas bibliotecas.</p><div class=\"countdown\">Esta janela tentará fechar em <strong id=\"seconds\">5</strong>s.</div><p class=\"hint\">Se ela não fechar automaticamente, pode fechá-la com segurança.</p></main><script>let seconds=5;const label=document.getElementById('seconds');const timer=setInterval(()=>{seconds-=1;label.textContent=seconds;if(seconds<=0){clearInterval(timer);window.close()}},1000)</script></body></html>";
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
            "A janela principal não pode ser ocultada pelo painel de diagnóstico.".to_string(),
        );
    }
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("A WebView '{label}' não está ativa."))?;
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
                    &format!(
                        "YouTube login page load: {:?} {}",
                        payload.event(),
                        payload.url()
                    ),
                );
            })
            .on_document_title_changed(|window, title| {
                write_native_log(
                    window.app_handle(),
                    &format!("YouTube login title: {title}"),
                );
            })
            .build()
        {
            Ok(window) => window,
            Err(error) => {
                write_native_log(
                    &app,
                    &format!("Failed to build YouTube login webview: {error}"),
                );
                return Err(error.to_string());
            }
        };
    write_native_log(&app, "Navigating YouTube login webview");
    window.navigate(url).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
async fn capture_youtube_session(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri::Manager;
    let window = app
        .get_webview_window("youtube-auth")
        .ok_or_else(|| "A janela de login do YouTube Music não está aberta.".to_string())?;
    let cookies = tauri::async_runtime::spawn_blocking(move || window.cookies())
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    let cookie_header = cookies
        .iter()
        .filter(|cookie| {
            cookie
                .domain()
                .is_some_and(|domain| domain.trim_start_matches('.').ends_with("youtube.com"))
        })
        .filter(|cookie| {
            cookie.name() == "SAPISID"
                || cookie.name() == "__Secure-1PAPISID"
                || cookie.name() == "__Secure-3PAPISID"
                || cookie.name() == "APISID"
                || cookie.name() == "SID"
                || cookie.name() == "SSID"
                || cookie.name() == "HSID"
                || cookie.name() == "LOGIN_INFO"
                || cookie.name() == "VISITOR_INFO1_LIVE"
                || cookie.name() == "PREF"
        })
        .fold(std::collections::BTreeMap::new(), |mut unique, cookie| {
            unique.insert(cookie.name(), cookie.value());
            unique
        })
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    if !cookie_header
        .split(';')
        .any(|cookie| cookie.trim_start().starts_with("SAPISID="))
    {
        return Ok(false);
    }
    let protected = protect_data(cookie_header.into_bytes(), false)?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("youtube-session.bin");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(path, protected).map_err(|error| error.to_string())?;
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
    Ok(path.exists())
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
        .ok_or_else(|| "GOOGLE_CLIENT_SECRET não foi configurado antes do build.".to_string())?;
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

#[cfg(windows)]
#[tauri::command]
async fn resolve_youtube_audio(
    app: tauri::AppHandle,
    video_id: String,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    write_native_log(
        &app,
        &format!("Resolving online audio for video {video_id}"),
    );
    let session_path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("youtube-session.bin");
    let protected_session = std::fs::read(session_path)
        .map_err(|_| "Faça login no YouTube Music antes de reproduzir uma faixa.".to_string())?;
    let cookie_header = String::from_utf8(protect_data(protected_session, true)?)
        .map_err(|error| error.to_string())?;
    let sapisid = cookie_value(&cookie_header, "SAPISID")
        .ok_or_else(|| "A sessão do YouTube Music não contém o cookie SAPISID.".to_string())?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let authorization = build_sapisid_authorization(&cookie_header, timestamp, &sapisid)?;
    let visitor_response = reqwest::Client::new()
        .get("https://music.youtube.com")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let visitor_html = visitor_response
        .text()
        .await
        .map_err(|error| error.to_string())?;
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
    write_native_log(
        &app,
        &format!(
            "InnerTube config: visitor={}, client_version={}, api_key={}",
            !visitor_id.is_empty(),
            client_version,
            !api_key.is_empty()
        ),
    );
    let body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": client_version,
                "hl": "en",
                "gl": "US",
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
    let endpoint = if api_key.is_empty() {
        "https://music.youtube.com/youtubei/v1/player?alt=json".to_string()
    } else {
        format!("https://music.youtube.com/youtubei/v1/player?alt=json&key={api_key}")
    };
    let response = reqwest::Client::new()
        .post(endpoint)
        .header("Authorization", authorization)
        .header("Cookie", cookie_header)
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
        .header("X-Goog-Visitor-Id", visitor_id)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .json(&body)
        .send()
        .await
        .map_err(|error| {
            write_native_log(&app, &format!("Player request failed: {error}"));
            error.to_string()
        })?;
    let status = response.status();
    let payload = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| error.to_string())?;
    if !status.is_success() {
        write_native_log(
            &app,
            &format!(
                "Player returned HTTP {status}: {}",
                payload
                    .get("error")
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "unknown error".to_string())
            ),
        );
        return Err(payload.to_string());
    }
    write_native_log(
        &app,
        &format!(
            "Player status: {}",
            payload
                .get("playabilityStatus")
                .and_then(|value| value.get("status"))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
        ),
    );
    let formats = payload
        .get("streamingData")
        .and_then(|data| data.get("adaptiveFormats"))
        .and_then(|formats| formats.as_array())
        .ok_or_else(|| "O YouTube não retornou formatos de áudio para esta faixa.".to_string())?;
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
            "Nenhum stream de áudio direto foi disponibilizado.".to_string()
        })?;
    write_native_log(&app, "Online audio stream resolved successfully");
    Ok(
        serde_json::json!({ "url": selected.get("url"), "mimeType": selected.get("mimeType"), "bitrate": selected.get("bitrate"), "expiresInSeconds": payload.get("streamingData").and_then(|data| data.get("expiresInSeconds")), "durationSeconds": payload.get("videoDetails").and_then(|details| details.get("lengthSeconds")) }),
    )
}

#[cfg(windows)]
#[tauri::command]
async fn search_youtube_music(
    app: tauri::AppHandle,
    query: String,
) -> Result<Vec<serde_json::Value>, String> {
    use tauri::Manager;
    write_native_log(&app, &format!("Searching YouTube Music: {}", query));
    let session_path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("youtube-session.bin");
    let protected_session = std::fs::read(session_path)
        .map_err(|_| "Faça login no YouTube Music antes de pesquisar.".to_string())?;
    let cookie_header = String::from_utf8(protect_data(protected_session, true)?)
        .map_err(|error| error.to_string())?;
    let sapisid = cookie_value(&cookie_header, "SAPISID")
        .ok_or_else(|| "A sessão do YouTube Music não contém o cookie SAPISID.".to_string())?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let authorization = build_sapisid_authorization(&cookie_header, timestamp, &sapisid)?;
    let visitor_response = reqwest::Client::new()
        .get("https://music.youtube.com")
        .header("Cookie", &cookie_header)
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
    let response = reqwest::Client::new()
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
        write_native_log(&app, &format!("YouTube search HTTP {status}: {payload}"));
        return Err(format!("YouTube search HTTP {status}: {payload}"));
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
#[tauri::command]
async fn load_youtube_playlists(app: tauri::AppHandle) -> Result<Vec<serde_json::Value>, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    let _library_lock = YOUTUBE_LIBRARY_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    write_native_log(&app, "Starting YouTube library bridge");
    let result = YOUTUBE_LIBRARY_BRIDGE.get_or_init(|| Mutex::new(None));
    *result.lock().map_err(|error| error.to_string())? = None;
    let existing_bridge = app.get_webview_window("youtube-library-bridge");
    let interceptor = r#"(() => { window.addEventListener('beforeunload', (event) => event.stopImmediatePropagation(), true); let sequence = 0; const metadata = (url, kind) => ({ requestId: ++sequence, playlistId: String(window.__vibeDeckExpectedPlaylistId || ''), kind, url: String(url), startedAt: performance.now() }); const save = (url, payload, meta) => { const value = String(url); if (value.includes('/youtubei/v1/browse')) { const currentPlaylist = String(window.__vibeDeckExpectedPlaylistId || ''); if (meta?.playlistId && currentPlaylist && meta.playlistId !== currentPlaylist) return; const serialized = JSON.stringify(payload); const isPlaylistShelf = serialized.includes('musicPlaylistShelfRenderer') || serialized.includes('musicPlaylistShelfContinuation'); const responseMeta = { ...meta, receivedAt: performance.now(), isPlaylistShelf }; window.__vibeDeckLastBrowseMetadata = responseMeta; window.__vibeDeckBrowseResponse = payload; if (isPlaylistShelf && !window.__vibeDeckInitialBrowsePayload) window.__vibeDeckInitialBrowsePayload = payload; } if (value.includes('/youtubei/v1/player')) window.__vibeDeckPlayerResponse = payload; }; const fetch = window.fetch; window.fetch = async (...args) => { const url = args[0]?.url || args[0]; const body = args[1]?.body; const kind = typeof body === 'string' && body.includes('continuation') ? 'continuation' : 'initial'; const meta = metadata(url, kind); const response = await fetch(...args); try { response.clone().json().then((payload) => save(url, payload, meta)); } catch (_) {} return response; }; const open = XMLHttpRequest.prototype.open; const send = XMLHttpRequest.prototype.send; XMLHttpRequest.prototype.open = function(method, url, ...args) { this.__vibeDeckUrl = url; this.__vibeDeckMetadata = metadata(url, 'initial'); return open.call(this, method, url, ...args); }; XMLHttpRequest.prototype.send = function(...args) { const body = args[0]; if (typeof body === 'string' && body.includes('continuation')) this.__vibeDeckMetadata.kind = 'continuation'; this.addEventListener('loadend', () => { try { save(this.__vibeDeckUrl, JSON.parse(this.responseText), this.__vibeDeckMetadata); } catch (_) {} }); return send.call(this, ...args); }; })();"#;
    let browse_response_guard = r#"(() => { let response = null; let pending = []; let timer = null; Object.defineProperty(window, '__vibeDeckBrowseResponse', { configurable: true, get: () => response, set: (payload) => { if (payload === null) { clearTimeout(timer); response = null; pending = []; return; } if (!location.pathname.includes('/playlist')) { response = payload; return; } const serialized = JSON.stringify(payload); if (!serialized.includes('musicResponsiveListItemRenderer') || !serialized.includes('videoId')) return; pending.push(payload); clearTimeout(timer); timer = setTimeout(() => { response = pending.slice(); }, 3500); } }); })();"#;
    let initial_api_interceptor = r#"(() => {})();"#;
    let playlist_payload_scope = r#"(() => { const find = (value, key) => { if (!value || typeof value !== 'object') return null; if (Array.isArray(value)) return value.map((item) => find(item, key)).find(Boolean); if (typeof value[key] === 'string') return value[key]; return Object.values(value).map((item) => find(item, key)).find(Boolean); }; const scope = (payload, continuation = false) => { if (!payload || !location.pathname.includes('/playlist')) return payload; const expected = String(window.__vibeDeckExpectedPlaylistId || ''); const shelves = []; const visit = (value) => { if (!value || typeof value !== 'object') return; const shelf = value.musicPlaylistShelfRenderer || value.musicPlaylistShelfContinuation; if (shelf?.contents) shelves.push(shelf); Object.values(value).forEach(visit); }; visit(payload); const shelf = shelves.toSorted((left, right) => (right.contents?.length || 0) - (left.contents?.length || 0))[0]; if (!shelf) return payload; const filtered = shelf.contents.filter((item) => { const row = item?.musicResponsiveListItemRenderer; if (!row || !expected) return true; const playlistId = find(row, 'playlistId'); return !playlistId || playlistId === expected || playlistId === `VL${expected}`; }); const contents = filtered.length ? filtered : shelf.contents; return { musicPlaylistShelfRenderer: { ...shelf, contents } }; }; window.__vibeDeckScopePlaylistPayload = scope; let initial = null; Object.defineProperty(window, '__vibeDeckInitialBrowsePayload', { configurable: true, get: () => initial, set: (value) => { initial = scope(value); } }); const browse = Object.getOwnPropertyDescriptor(window, '__vibeDeckBrowseResponse'); if (browse?.get && browse?.set) Object.defineProperty(window, '__vibeDeckBrowseResponse', { configurable: true, get: browse.get, set: (value) => browse.set(scope(value)) }); })();"#;
    let continuation_paginator = r#"(() => { const findTokens = (value, tokens = []) => { if (!value || typeof value !== 'object') return tokens; if (Array.isArray(value)) { value.forEach((item) => findTokens(item, tokens)); return tokens; } const continuation = value.continuationItemRenderer?.continuationEndpoint?.continuationCommand?.token || value.nextContinuationData?.continuation; if (typeof continuation === 'string') tokens.push(continuation); Object.values(value).forEach((item) => findTokens(item, tokens)); return tokens; }; window.__vibeDeckContinuationSeen = new Set(); window.__vibeDeckContinuationPending = []; window.__vibeDeckLoadNextPlaylistPage = async () => { const scope = window.__vibeDeckScopePlaylistPayload || ((value) => value); const initial = window.__vibeDeckInitialBrowsePayload; if (initial && !window.__vibeDeckContinuationPending.length) window.__vibeDeckContinuationPending.push(...findTokens(scope(initial, true))); const key = window.ytcfg?.get?.('INNERTUBE_API_KEY'); const context = window.ytcfg?.get?.('INNERTUBE_CONTEXT'); if (!key || !context) return false; while (window.__vibeDeckContinuationPending.length) { const token = window.__vibeDeckContinuationPending.shift(); if (!token || window.__vibeDeckContinuationSeen.has(token)) continue; window.__vibeDeckContinuationSeen.add(token); const response = await fetch(`/youtubei/v1/browse?key=${encodeURIComponent(key)}&prettyPrint=false`, { method: 'POST', credentials: 'include', headers: { 'content-type': 'application/json', 'x-youtube-client-name': String(window.ytcfg.get('INNERTUBE_CONTEXT_CLIENT_NAME') || 67), 'x-youtube-client-version': String(window.ytcfg.get('INNERTUBE_CONTEXT_CLIENT_VERSION') || '') }, body: JSON.stringify({ context, continuation: token }) }); if (!response.ok) return false; const payload = await response.json(); const scoped = scope(payload, true); findTokens(scoped).forEach((next) => { if (!window.__vibeDeckContinuationSeen.has(next)) window.__vibeDeckContinuationPending.push(next); }); window.__vibeDeckBrowseResponse = null; window.__vibeDeckBrowseResponse = scoped; return true; } return false; }; })();"#;
    let playlist_action_scope = r#"(() => { const previous = window.__vibeDeckScopePlaylistPayload || ((value) => value); const normalize = (value) => String(value || '').replace(/^VL/, ''); const scope = (payload, continuation = false) => { const expected = normalize(window.__vibeDeckExpectedPlaylistId); let found = null; const visit = (value) => { if (!value || typeof value !== 'object' || found) return; if (Array.isArray(value)) return value.forEach(visit); const action = value.appendContinuationItemsAction; const target = normalize(action?.targetId); if (action?.continuationItems && (!expected || !target || target === expected)) found = action.continuationItems; Object.values(value).forEach(visit); }; visit(payload); return found ? { musicPlaylistShelfRenderer: { contents: found } } : previous(payload, continuation); }; window.__vibeDeckScopePlaylistPayload = scope; let initial = null; Object.defineProperty(window, '__vibeDeckInitialBrowsePayload', { configurable: true, get: () => initial, set: (value) => { initial = scope(value); } }); const browse = Object.getOwnPropertyDescriptor(window, '__vibeDeckBrowseResponse'); if (browse?.get && browse?.set) Object.defineProperty(window, '__vibeDeckBrowseResponse', { configurable: true, get: browse.get, set: (value) => browse.set(scope(value)) }); })();"#;
    let dom_continuation_fallback = r#"(() => { const previous = window.__vibeDeckLoadNextPlaylistPage; const duration = (value) => /^\d+:\d{2}(?::\d{2})?$/.test(value); const domPayload = () => { const expected = String(window.__vibeDeckExpectedPlaylistId || '').replace(/^VL/, ''); const contents = [...document.querySelectorAll('ytmusic-responsive-list-item-renderer')].map((row) => { const href = row.querySelector('a[href*="watch?v="]')?.getAttribute('href') || ''; const params = new URLSearchParams(href.split('?')[1] || ''); const videoId = params.get('v'); if (expected && params.get('list') !== expected) return null; const lines = row.innerText.split('\n').map((value) => value.trim()).filter(Boolean); const title = lines[0]; const artist = lines[1] || 'YouTube'; const durationText = lines.find(duration) || '--:--'; const coverUrl = row.querySelector('img')?.getAttribute('src'); if (!videoId || !title) return null; return { musicResponsiveListItemRenderer: { videoId, flexColumns: [{ musicResponsiveListItemFlexColumnRenderer: { text: { runs: [{ text: title }] } } }, { musicResponsiveListItemFlexColumnRenderer: { text: { runs: [{ text: artist }] } } }], fixedColumns: [{ musicResponsiveListItemFixedColumnRenderer: { text: { runs: [{ text: durationText }] } } }], thumbnail: { musicThumbnailRenderer: { thumbnail: { thumbnails: coverUrl ? [{ url: coverUrl }] : [] } } } } }; }).filter(Boolean); return contents.length ? { musicPlaylistShelfRenderer: { contents } } : null; }; window.__vibeDeckLoadNextPlaylistPage = async () => { if (await previous?.()) return true; window.scrollTo(0, document.documentElement.scrollHeight); await new Promise((resolve) => setTimeout(resolve, 1800)); const payload = domPayload(); if (!payload) return false; window.__vibeDeckBrowseResponse = null; window.__vibeDeckBrowseResponse = payload; return true; }; })();"#;
    let media_blocker = r#"(() => { const block = (media) => { if (!(media instanceof HTMLMediaElement)) return; media.autoplay = false; media.removeAttribute('autoplay'); media.muted = true; media.defaultMuted = true; media.volume = 0; media.pause(); }; HTMLMediaElement.prototype.play = function() { block(this); return Promise.resolve(); }; new MutationObserver((records) => records.forEach((record) => record.addedNodes.forEach((node) => { if (node instanceof HTMLMediaElement) block(node); if (node instanceof Element) node.querySelectorAll?.('audio, video').forEach(block); }))).observe(document, { childList: true, subtree: true }); document.querySelectorAll?.('audio, video').forEach(block); })();"#;
    let chunked_title_bridge = r#"(() => { const descriptor = Object.getOwnPropertyDescriptor(Document.prototype, 'title'); if (!descriptor?.get || !descriptor?.set) return; const source = []; let sourceRequest = null; Object.defineProperty(document, 'title', { configurable: true, get: () => descriptor.get.call(document), set: (value) => { const current = String(value); const match = /^__VIBEDECK_PLAYLIST_CHUNK__(?:(p\d+)_)?(\d+)_(\d+)_(.*)$/.exec(current); if (!match) return descriptor.set.call(document, value); const request = match[1] || window.__vibeDeckPlaylistRequestId || 'missing'; const index = Number(match[2]); const total = Number(match[3]); if (sourceRequest !== request || source.length !== total) { sourceRequest = request; source.length = total; source.fill(null); } source[index] = decodeURIComponent(match[4]); if (!source.every((chunk) => typeof chunk === 'string')) return; const json = source.join(''); const size = 500; const count = Math.ceil(json.length / size); for (let part = 0; part < count; part += 1) setTimeout(() => descriptor.set.call(document, `__VIBEDECK_PLAYLIST_CHUNK__${request}_${part}_${count}_${encodeURIComponent(json.slice(part * size, (part + 1) * size))}`), part * 20); } }); })();"#;
    let bridge = if let Some(window) = existing_bridge {
        window
    } else {
        WebviewWindowBuilder::new(&app, "youtube-library-bridge", WebviewUrl::App("index.html".into()))
        .title("VibeDeck - YouTube Music library bridge")
        .visible(false)
        .initialization_script(interceptor)
        .initialization_script(browse_response_guard)
        .initialization_script(initial_api_interceptor)
        .initialization_script(playlist_payload_scope)
        .initialization_script(continuation_paginator)
        .initialization_script(playlist_action_scope)
        .initialization_script(dom_continuation_fallback)
        .initialization_script(media_blocker)
        .initialization_script(chunked_title_bridge)
        .on_page_load(|window, payload| {
            write_native_log(window.app_handle(), &format!("YouTube library bridge page: {:?} {}", payload.event(), payload.url()));
            if payload.url().as_str().starts_with("https://music.youtube.com/library") {
                write_native_log(window.app_handle(), "YouTube library bridge page loaded");
                let script = r#"(() => { let attempts = 0; const find = (value, key) => { if (!value || typeof value !== 'object') return null; if (Array.isArray(value)) return value.map((item) => find(item, key)).find(Boolean); if (typeof value[key] === 'string') return value[key]; return Object.values(value).map((item) => find(item, key)).find(Boolean); }; const collect = () => { const payload = window.__vibeDeckBrowseResponse; if (!payload && attempts++ < 50) return setTimeout(collect, 150); const playlists = []; const visit = (value) => { if (!value || playlists.length >= 100) return; if (Array.isArray(value)) return value.forEach(visit); if (typeof value !== 'object') return; const row = value.musicTwoRowItemRenderer; if (row) { const id = find(row, 'browseId'); const name = row.title?.runs?.[0]?.text; if (id && name && id.startsWith('VL') && !playlists.some((item) => item.id === id)) playlists.push({ id, name, source: 'online', mode: 'online', tracks: [], allowLocal: false }); } Object.values(value).forEach(visit); }; visit(payload); document.title = '__VIBEDECK_LIBRARY__' + encodeURIComponent(JSON.stringify(playlists)); }; collect(); })();"#;
                if let Err(error) = window.eval(script) { write_native_log(window.app_handle(), &format!("YouTube library bridge script failed: {error}")); }
            }
            if payload.url().as_str().contains("music.youtube.com/playlist") {
                let diagnostics = r#"(() => { const report = (kind, value) => { const message = value?.message || String(value || 'unknown'); document.title = `__VIBEDECK_PLAYLIST_DEBUG__${encodeURIComponent(`${kind}: ${message}`)}`; }; window.addEventListener('error', (event) => report('error', event.error || event.message), { once: true }); window.addEventListener('unhandledrejection', (event) => report('rejection', event.reason), { once: true }); })();"#;
                if let Err(error) = window.eval(diagnostics) {
                    write_native_log(window.app_handle(), &format!("YouTube playlist bridge diagnostics injection failed: {error}"));
                }
                let script = r#"(() => { let attempts = 0; const find = (value, key) => { if (!value || typeof value !== 'object') return null; if (Array.isArray(value)) return value.map((item) => find(item, key)).find(Boolean); if (typeof value[key] === 'string') return value[key]; return Object.values(value).map((item) => find(item, key)).find(Boolean); }; const duration = (value) => /^\d+:\d{2}(?::\d{2})?$/.test(value); const collect = () => { const payload = window.__vibeDeckBrowseResponse; if (!payload && attempts++ < 50) return setTimeout(collect, 150); const tracks = []; const visit = (value) => { if (!value || tracks.length >= 1000) return; if (Array.isArray(value)) return value.forEach(visit); if (typeof value !== 'object') return; const row = value.musicResponsiveListItemRenderer; if (row) { const videoId = find(row, 'videoId'); const title = row.flexColumns?.[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text; const meta = (row.flexColumns?.[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []).map((run) => run.text).filter((text) => text && text !== ' • ' && text !== 'Música'); const artist = meta.find((text) => !duration(text)) || 'YouTube'; const durationText = row.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || meta.find(duration) || '--:--'; const coverUrl = row.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails?.at(-1)?.url; if (videoId && title && !tracks.some((item) => item.youtubeId === videoId)) tracks.push({ id: `youtube-${videoId}`, youtubeId: videoId, title, artist, album: 'YouTube Music', duration: durationText, coverUrl, source: 'youtube', accent: '#ff5f55' }); } Object.values(value).forEach(visit); }; visit(payload); const json = JSON.stringify(tracks); const size = 6000; const total = Math.max(1, Math.ceil(json.length / size)); for (let index = 0; index < total; index += 1) setTimeout(() => { document.title = `__VIBEDECK_PLAYLIST_CHUNK__${index}_${total}_${encodeURIComponent(json.slice(index * size, (index + 1) * size))}`; }, index * 20); }; collect(); })();"#;
                 match window.eval(script) {
                     Ok(()) => write_native_log(window.app_handle(), "YouTube playlist bridge collector injected"),
                     Err(error) => write_native_log(window.app_handle(), &format!("YouTube playlist bridge collector injection failed: {error}")),
                 }
                 let embedded_browse_bridge = r#"(() => { let attempts = 0; const decode = (value) => { try { return Function(`"use strict"; return '${value}'`)(); } catch (_) { return null; } }; const collect = () => { const source = [...document.scripts].map((script) => script.textContent || '').find((text) => text.includes('initialData.push') && text.includes('/browse')); const match = source?.match(/initialData\.push\(\{\s*path:\s*['"]\\?\/browse['"][\s\S]*?data:\s*'((?:\\.|[^'])*)'/); const decoded = match ? decode(match[1]) : null; if (!decoded) { if (attempts++ < 80) return setTimeout(collect, 150); return; } try { const payload = JSON.parse(decoded); window.__vibeDeckInitialBrowsePayload = payload; window.__vibeDeckBrowseResponse = payload; } catch (_) {} }; collect(); })();"#;
                 if let Err(error) = window.eval(embedded_browse_bridge) {
                     write_native_log(window.app_handle(), &format!("YouTube embedded browse bridge injection failed: {error}"));
                 }
                 let dom_fallback = r#"(() => { setTimeout(() => { const expected = String(window.__vibeDeckExpectedPlaylistId || '').replace(/^VL/, ''); const duration = (value) => /^\d+:\d{2}(?::\d{2})?$/.test(value); const tracks = [...document.querySelectorAll('ytmusic-responsive-list-item-renderer')].map((row) => { const href = row.querySelector('a[href*="watch?v="]')?.getAttribute('href') || ''; const params = new URLSearchParams(href.split('?')[1] || ''); const videoId = params.get('v'); if (expected && params.get('list') !== expected) return null; const lines = row.innerText.split('\n').map((value) => value.trim()).filter(Boolean); const durationText = lines.find(duration) || '--:--'; const title = lines[0]; const artist = lines[1] || 'YouTube'; const coverUrl = row.querySelector('img')?.getAttribute('src'); return videoId && title ? { id: `youtube-${videoId}`, youtubeId: videoId, title, artist, album: 'YouTube Music', duration: durationText, coverUrl, source: 'youtube', accent: '#ff5f55' } : null; }).filter(Boolean); if (!tracks.length) return; const json = JSON.stringify(tracks); const size = 6000; const total = Math.max(1, Math.ceil(json.length / size)); for (let index = 0; index < total; index += 1) setTimeout(() => { document.title = `__VIBEDECK_PLAYLIST_CHUNK__${index}_${total}_${encodeURIComponent(json.slice(index * size, (index + 1) * size))}`; }, index * 25); }, 4500); })();"#;
                 if let Err(error) = window.eval(dom_fallback) {
                     write_native_log(window.app_handle(), &format!("YouTube playlist DOM fallback injection failed: {error}"));
                 }
                 let dom_retry = r#"(() => { let attempts = 0; const duration = (value) => /^\d+:\d{2}(?::\d{2})?$/.test(value); const collect = () => { const expected = String(window.__vibeDeckExpectedPlaylistId || '').replace(/^VL/, ''); const tracks = [...document.querySelectorAll('ytmusic-responsive-list-item-renderer')].map((row) => { const href = row.querySelector('a[href*="watch?v="]')?.getAttribute('href') || ''; const params = new URLSearchParams(href.split('?')[1] || ''); const videoId = params.get('v'); if (expected && params.get('list') !== expected) return null; const lines = row.innerText.split('\n').map((value) => value.trim()).filter(Boolean); const title = lines[0]; if (!videoId || !title) return null; return { id: `youtube-${videoId}`, youtubeId: videoId, title, artist: lines[1] || 'YouTube', album: 'YouTube Music', duration: lines.find(duration) || '--:--', coverUrl: row.querySelector('img')?.getAttribute('src'), source: 'youtube', accent: '#ff5f55' }; }).filter(Boolean); if (!tracks.length && attempts++ < 240) return setTimeout(collect, 150); if (!tracks.length) return; const json = JSON.stringify(tracks); const size = 6000; const total = Math.max(1, Math.ceil(json.length / size)); for (let index = 0; index < total; index += 1) setTimeout(() => { document.title = `__VIBEDECK_PLAYLIST_CHUNK__${index}_${total}_${encodeURIComponent(json.slice(index * size, (index + 1) * size))}`; }, index * 25); }; collect(); })();"#;
                 if let Err(error) = window.eval(dom_retry) {
                     write_native_log(window.app_handle(), &format!("YouTube playlist DOM retry injection failed: {error}"));
                 }
             }
             if payload.url().as_str().contains("music.youtube.com/watch") {
                let script = r#"(() => { let attempts = 0; let requested = false; const isAudio = (url) => /(?:mime=audio|mime=audio%2F|type=audio|audio_only)/i.test(url) && !/mime=video/i.test(url); const collect = () => { const resource = performance.getEntriesByType('resource').map((entry) => entry.name).reverse().find((url) => url.includes('googlevideo.com/videoplayback') && isAudio(url)); if (resource) { document.title = '__VIBEDECK_STREAM__' + encodeURIComponent(resource); return; } const video = document.querySelector('video'); if (video && !requested) { requested = true; video.muted = true; video.volume = 0; video.pause(); video.preload = 'auto'; video.load(); } if (attempts++ < 80) setTimeout(collect, 150); }; collect(); })();"#;
                let _ = window.eval(script);
            }
        })
        .on_document_title_changed(|window, title| {
            if let Some(encoded) = title.strip_prefix("__VIBEDECK_LIBRARY__") {
                let decoded = url::form_urlencoded::parse(format!("value={encoded}").as_bytes()).next().map(|(_, value)| value.into_owned());
                if let Some(value) = decoded {
                if let Some(result) = YOUTUBE_LIBRARY_BRIDGE.get() {
                    if let Ok(mut stored) = result.lock() {
                        *stored = Some(value);
                        write_native_log(window.app_handle(), "YouTube library bridge received DOM data");
                    }
                }
                }
            }
            if let Some(encoded) = title.strip_prefix("__VIBEDECK_PLAYLIST__") {
                if let Some((_, value)) = url::form_urlencoded::parse(format!("value={encoded}").as_bytes()).next() {
                if let Some(result) = YOUTUBE_PLAYLIST_BRIDGE.get() {
                    if let Ok(mut stored) = result.lock() { *stored = Some(value.into_owned()); }
                }
                }
            }
            if let Some(chunk) = title.strip_prefix("__VIBEDECK_PLAYLIST_CHUNK__") {
                let active_request = YOUTUBE_PLAYLIST_REQUEST.get_or_init(|| Mutex::new(String::new()));
                let normalized = chunk.trim_start_matches('_');
                let parts = normalized.splitn(4, '_').collect::<Vec<_>>();
                let (request_id, index, total, encoded) = match parts.as_slice() {
                    [request_id, index, total, encoded] if request_id.starts_with('p') => (request_id.to_string(), *index, *total, *encoded),
                    [_bridge_request, index, total, encoded] => (active_request.lock().map(|active| active.clone()).unwrap_or_default(), *index, *total, *encoded),
                    [index, total, encoded] => (active_request.lock().map(|active| active.clone()).unwrap_or_default(), *index, *total, *encoded),
                    _ => {
                        let logged = MALFORMED_PLAYLIST_CHUNK_LOGGED.get_or_init(|| Mutex::new(false));
                        if let Ok(mut logged) = logged.lock() {
                            if !*logged {
                                *logged = true;
                                let preview = normalized.chars().take(180).collect::<String>();
                                write_native_log(window.app_handle(), &format!("YouTube playlist bridge ignored malformed chunk title once: preview={preview}"));
                            }
                        }
                        return;
                    }
                };
                if active_request.lock().map(|active| active.as_str() != request_id.as_str()).unwrap_or(true) { return; }
                let (Ok(index), Ok(total)) = (index.parse::<usize>(), total.parse::<usize>()) else { return; };
                let value = url::form_urlencoded::parse(format!("value={encoded}").as_bytes()).next().map(|(_, value)| value.into_owned());
                if let Some(value) = value {
                    let chunks = YOUTUBE_PLAYLIST_CHUNKS.get_or_init(|| Mutex::new(Vec::new()));
                    if let Ok(mut chunks) = chunks.lock() {
                        if chunks.len() != total { *chunks = (0..total).map(|_| None).collect(); }
                        if index < chunks.len() { chunks[index] = Some(value); }
                        if chunks.iter().all(Option::is_some) {
                            let joined = chunks.iter().filter_map(|item| item.as_deref()).collect::<String>();
                            if serde_json::from_str::<Vec<serde_json::Value>>(&joined).is_ok() {
                                write_native_log(window.app_handle(), &format!("YouTube playlist bridge received complete chunk payload: request_id={request_id}, chunks={total}, bytes={}", joined.len()));
                                if let Some(result) = YOUTUBE_PLAYLIST_BRIDGE.get() {
                                    if let Ok(mut stored) = result.lock() { *stored = Some(joined); }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(request_id) = title.strip_prefix("__VIBEDECK_PLAYLIST_DONE__") {
                let active_request = YOUTUBE_PLAYLIST_REQUEST.get_or_init(|| Mutex::new(String::new()));
                if !request_id.is_empty() && active_request.lock().map(|active| active.as_str() != request_id).unwrap_or(true) { return; }
                if let Some(result) = YOUTUBE_PLAYLIST_BRIDGE.get() {
                    if let Ok(mut stored) = result.lock() { *stored = Some("[]".to_string()); }
                }
            }
            if let Some(encoded) = title.strip_prefix("__VIBEDECK_STREAM__") {
                if let Some((_, value)) = url::form_urlencoded::parse(format!("value={encoded}").as_bytes()).next() {
                    if let Some(result) = YOUTUBE_STREAM_BRIDGE.get() {
                        if let Ok(mut stored) = result.lock() { *stored = Some(value.into_owned()); }
                    }
                }
            }
        })
        .build()
        .map_err(|error| error.to_string())?
    };
    mute_webview(&bridge).map_err(|error| {
        write_native_log(
            &app,
            &format!("YouTube library bridge native mute failed: {error}"),
        );
        error
    })?;
    let session_path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("youtube-session.bin");
    let protected_session = std::fs::read(session_path)
        .map_err(|_| "Faça login no YouTube Music antes de carregar sua biblioteca.".to_string())?;
    let cookie_header = String::from_utf8(protect_data(protected_session, true)?)
        .map_err(|error| error.to_string())?;
    write_native_log(
        &app,
        &format!(
            "YouTube session restored: {} cookies",
            cookie_header
                .split(';')
                .filter(|cookie| !cookie.trim().is_empty())
                .count()
        ),
    );
    for cookie in cookie_header
        .split(';')
        .filter_map(|item| item.trim().split_once('='))
    {
        let cookie = tauri::webview::Cookie::build((cookie.0.to_string(), cookie.1.to_string()))
            .domain(".youtube.com")
            .path("/")
            .secure(true)
            .build();
        bridge
            .set_cookie(cookie)
            .map_err(|error| error.to_string())?;
    }
    write_native_log(&app, "Navigating YouTube library bridge");
    let url = "https://music.youtube.com/library"
        .parse()
        .map_err(|error: url::ParseError| error.to_string())?;
    bridge.navigate(url).map_err(|error| error.to_string())?;
    for _ in 0..240 {
        if let Some(value) = result.lock().map_err(|error| error.to_string())?.take() {
            let playlists = serde_json::from_str::<Vec<serde_json::Value>>(&value)
                .map_err(|error| error.to_string())?;
            write_native_log(
                &app,
                &format!(
                    "YouTube library bridge returned {} playlists",
                    playlists.len()
                ),
            );
            return Ok(playlists);
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    write_native_log(&app, "YouTube library bridge timed out");
    Err("Tempo limite ao carregar playlists do YouTube Music.".to_string())
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
    let protected_session = std::fs::read(session_path)
        .map_err(|_| "Faça login no YouTube Music antes de carregar a playlist.".to_string())?;
    let cookie_header = String::from_utf8(protect_data(protected_session, true)?)
        .map_err(|error| error.to_string())?;
    let html = reqwest::Client::new()
        .get(format!("https://music.youtube.com/playlist?list={list_id}"))
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
        let tracks = playlist_tracks_from_payload(&payload);
        if !tracks.is_empty() {
            let has_more = playlist_continuation_for(&payload, list_id).is_some();
            let continuations =
                YOUTUBE_PLAYLIST_CONTINUATIONS.get_or_init(|| Mutex::new(HashMap::new()));
            if let Ok(mut continuations) = continuations.lock() {
                if let Some(token) = playlist_continuation_for(&payload, list_id) {
                    continuations.insert(playlist_id.clone(), token);
                    if let Ok(mut seen) = YOUTUBE_PLAYLIST_SEEN_CONTINUATIONS
                        .get_or_init(|| Mutex::new(HashMap::new()))
                        .lock()
                    {
                        let mut tokens = HashSet::new();
                        tokens.insert(continuations[&playlist_id].clone());
                        seen.insert(playlist_id.clone(), tokens);
                    }
                } else {
                    // An empty marker distinguishes a terminal embedded page from bridge state.
                    continuations.insert(playlist_id.clone(), String::new());
                }
            }
            write_native_log(
                &app,
                &format!(
                    "YouTube embedded playlist returned {} tracks: playlist_id={playlist_id}",
                    tracks.len()
                ),
            );
            return Ok(YouTubePlaylistPage {
                tracks,
                has_more,
                continuation: playlist_continuation_state(&playlist_id),
            });
        }
    }
    write_native_log(&app, &format!("YouTube embedded playlist had no tracks, falling back to bridge: playlist_id={playlist_id}"));
    let result = YOUTUBE_PLAYLIST_BRIDGE.get_or_init(|| Mutex::new(None));
    let bridge = app
        .get_webview_window("youtube-library-bridge")
        .ok_or_else(|| {
            write_native_log(&app, &format!("YouTube playlist bridge unavailable while loading playlist tracks: playlist_id={playlist_id}"));
            "O broker do YouTube Music não está ativo. Sincronize a sessão novamente.".to_string()
        })?;
    let url: url::Url = format!("https://music.youtube.com/playlist?list={list_id}")
        .parse()
        .map_err(|error: url::ParseError| error.to_string())?;
    for attempt in 1..=2 {
        let request_id = format!(
            "p{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        );
        *YOUTUBE_PLAYLIST_REQUEST
            .get_or_init(|| Mutex::new(String::new()))
            .lock()
            .map_err(|error| error.to_string())? = request_id.clone();
        if let Ok(mut debug) = YOUTUBE_PLAYLIST_DEBUG
            .get_or_init(|| Mutex::new(PlaylistBridgeDebugState::default()))
            .lock()
        {
            debug.playlist_id = Some(playlist_id.clone());
            debug.request_id = Some(request_id.clone());
            debug.stage = "loading_initial".to_string();
            debug.initial_tracks = 0;
            debug.last_page_tracks = 0;
            debug.total_tracks = 0;
        }
        *result.lock().map_err(|error| error.to_string())? = None;
        YOUTUBE_PLAYLIST_CHUNKS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .map_err(|error| error.to_string())?
            .clear();
        if let Ok(mut logged) = MALFORMED_PLAYLIST_CHUNK_LOGGED
            .get_or_init(|| Mutex::new(false))
            .lock()
        {
            *logged = false;
        }
        write_native_log(
            &app,
            &format!("YouTube playlist bridge attempt started: playlist_id={playlist_id}, attempt={attempt}, request_id={request_id}"),
        );
        bridge.eval(&format!("window.__vibeDeckPlaylistRequestId = '{}'; window.__vibeDeckExpectedPlaylistId = '{}'; window.__vibeDeckBrowseResponse = null; window.__vibeDeckInitialBrowsePayload = null; window.__vibeDeckContinuationSeen = new Set(); window.__vibeDeckContinuationPending = [];", request_id, list_id)).map_err(|error| error.to_string())?;
        write_native_log(
            &app,
            &format!("YouTube playlist bridge state reset: playlist_id={playlist_id}, request_id={request_id}"),
        );
        bridge
            .navigate(url.clone())
            .map_err(|error| error.to_string())?;
        write_native_log(
            &app,
            &format!("YouTube playlist bridge navigating: playlist_id={playlist_id}, request_id={request_id}, url={url}"),
        );
        for _ in 0..240 {
            if let Some(value) = result.lock().map_err(|error| error.to_string())?.take() {
                let tracks =
                    serde_json::from_str::<Vec<serde_json::Value>>(&value).map_err(|error| {
                        write_native_log(
                            &app,
                            &format!("YouTube playlist bridge returned invalid JSON: {error}"),
                        );
                        error.to_string()
                    })?;
                write_native_log(
                    &app,
                    &format!("YouTube playlist bridge returned {} tracks: playlist_id={playlist_id}, request_id={request_id}", tracks.len()),
                );
                if let Ok(mut debug) = YOUTUBE_PLAYLIST_DEBUG
                    .get_or_init(|| Mutex::new(PlaylistBridgeDebugState::default()))
                    .lock()
                {
                    debug.stage = "initial_loaded".to_string();
                    debug.initial_tracks = tracks.len();
                    debug.last_page_tracks = tracks.len();
                    debug.total_tracks = tracks.len();
                }
                return Ok(YouTubePlaylistPage {
                    tracks,
                    has_more: true,
                    continuation: playlist_continuation_state(&playlist_id),
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
        write_native_log(
            &app,
            &format!("YouTube playlist bridge attempt {attempt} timed out: playlist_id={playlist_id}, request_id={request_id}"),
        );
    }
    write_native_log(
        &app,
        "YouTube playlist bridge timed out while loading tracks",
    );
    Err("Tempo limite ao carregar as faixas da playlist.".to_string())
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
        let protected_session = std::fs::read(session_path)
            .map_err(|_| "Faça login no YouTube Music antes de carregar a playlist.".to_string())?;
        let cookie_header = String::from_utf8(protect_data(protected_session, true)?)
            .map_err(|error| error.to_string())?;
        let sapisid = cookie_value(&cookie_header, "SAPISID")
            .ok_or_else(|| "A sessão não contém SAPISID.".to_string())?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default();
        let authorization = build_sapisid_authorization(&cookie_header, timestamp, &sapisid)?;
        let user_agent = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153.0.0.0 Safari/537.36";
        let config_html = reqwest::Client::new()
            .get("https://music.youtube.com")
            .header("Cookie", &cookie_header)
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
        let response = reqwest::Client::new()
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
    let result = YOUTUBE_PLAYLIST_BRIDGE.get_or_init(|| Mutex::new(None));
    let request_id = format!(
        "p{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    );
    *YOUTUBE_PLAYLIST_REQUEST
        .get_or_init(|| Mutex::new(String::new()))
        .lock()
        .map_err(|error| error.to_string())? = request_id.clone();
    if let Ok(mut debug) = YOUTUBE_PLAYLIST_DEBUG
        .get_or_init(|| Mutex::new(PlaylistBridgeDebugState::default()))
        .lock()
    {
        debug.playlist_id = Some(playlist_id.clone());
        debug.request_id = Some(request_id.clone());
        debug.stage = "loading_continuation".to_string();
    }
    *result.lock().map_err(|error| error.to_string())? = None;
    YOUTUBE_PLAYLIST_CHUNKS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|error| error.to_string())?
        .clear();
    if let Ok(mut logged) = MALFORMED_PLAYLIST_CHUNK_LOGGED
        .get_or_init(|| Mutex::new(false))
        .lock()
    {
        *logged = false;
    }
    let bridge = app
        .get_webview_window("youtube-library-bridge")
        .ok_or_else(|| {
            write_native_log(&app, &format!("YouTube playlist bridge unavailable while loading more tracks: playlist_id={playlist_id}, request_id={request_id}"));
            "O broker do YouTube Music não está ativo. Sincronize a sessão novamente.".to_string()
        })?;
    write_native_log(
        &app,
        &format!("YouTube playlist bridge more tracks request started: playlist_id={playlist_id}, request_id={request_id}"),
    );
    bridge
        .eval(&format!(
            "window.__vibeDeckPlaylistRequestId = '{}';",
            request_id
        ))
        .map_err(|error| error.to_string())?;
    write_native_log(
        &app,
        &format!("YouTube playlist bridge more tracks request id injected: playlist_id={playlist_id}, request_id={request_id}"),
    );
    let script = r#"(() => { const find = (value, key) => { if (!value || typeof value !== 'object') return null; if (Array.isArray(value)) return value.map((item) => find(item, key)).find(Boolean); if (typeof value[key] === 'string') return value[key]; return Object.values(value).map((item) => find(item, key)).find(Boolean); }; const duration = (value) => /^\d+:\d{2}(?::\d{2})?$/.test(value); const emit = (tracks) => { if (!tracks.length) { document.title = '__VIBEDECK_PLAYLIST_DONE__'; return; } const json = JSON.stringify(tracks); const size = 6000; const total = Math.max(1, Math.ceil(json.length / size)); for (let index = 0; index < total; index += 1) setTimeout(() => { document.title = `__VIBEDECK_PLAYLIST_CHUNK__${index}_${total}_${encodeURIComponent(json.slice(index * size, (index + 1) * size))}`; }, index * 25); }; const collect = async () => { const ok = await window.__vibeDeckLoadNextPlaylistPage?.(); if (!ok) return emit([]); await new Promise((resolve) => setTimeout(resolve, 3600)); const payload = window.__vibeDeckBrowseResponse; const tracks = []; const visit = (value) => { if (!value || tracks.length >= 1000) return; if (Array.isArray(value)) return value.forEach(visit); if (typeof value !== 'object') return; const row = value.musicResponsiveListItemRenderer; if (row) { const videoId = find(row, 'videoId'); const title = row.flexColumns?.[0]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs?.[0]?.text; const meta = (row.flexColumns?.[1]?.musicResponsiveListItemFlexColumnRenderer?.text?.runs || []).map((run) => run.text).filter((text) => text && text !== ' • ' && text !== 'Música'); const artist = meta.find((text) => !duration(text)) || 'YouTube'; const durationText = row.fixedColumns?.[0]?.musicResponsiveListItemFixedColumnRenderer?.text?.runs?.[0]?.text || meta.find(duration) || '--:--'; const coverUrl = row.thumbnail?.musicThumbnailRenderer?.thumbnail?.thumbnails?.at(-1)?.url; if (videoId && title && !tracks.some((item) => item.youtubeId === videoId)) tracks.push({ id: `youtube-${videoId}`, youtubeId: videoId, title, artist, album: 'YouTube Music', duration: durationText, coverUrl, source: 'youtube', accent: '#ff5f55' }); } Object.values(value).forEach(visit); }; visit(payload); emit(tracks); }; collect(); })();"#;
    bridge.eval(script).map_err(|error| error.to_string())?;
    for _ in 0..80 {
        if let Some(value) = result.lock().map_err(|error| error.to_string())?.take() {
            let tracks = serde_json::from_str::<Vec<serde_json::Value>>(&value)
                .map_err(|error| error.to_string())?;
            write_native_log(
                &app,
                &format!(
                    "YouTube playlist bridge returned {} more tracks: playlist_id={playlist_id}, request_id={request_id}",
                    tracks.len()
                ),
            );
            if let Ok(mut debug) = YOUTUBE_PLAYLIST_DEBUG
                .get_or_init(|| Mutex::new(PlaylistBridgeDebugState::default()))
                .lock()
            {
                debug.stage = if tracks.is_empty() {
                    "continuation_empty"
                } else {
                    "continuation_loaded"
                }
                .to_string();
                debug.last_page_tracks = tracks.len();
                debug.total_tracks += tracks.len();
            }
            return Ok(YouTubePlaylistPage {
                tracks,
                has_more: true,
                continuation: playlist_continuation_state(&playlist_id),
            });
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    write_native_log(
        &app,
        &format!("YouTube playlist bridge timed out while loading more tracks: playlist_id={playlist_id}, request_id={request_id}"),
    );
    Err("Tempo limite ao carregar mais faixas da playlist.".to_string())
}

#[cfg(windows)]
#[tauri::command]
async fn resolve_youtube_audio_bridge(
    app: tauri::AppHandle,
    video_id: String,
) -> Result<serde_json::Value, String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    let result = YOUTUBE_STREAM_BRIDGE.get_or_init(|| Mutex::new(None));
    *result.lock().map_err(|error| error.to_string())? = None;
    let bridge = if let Some(window) = app.get_webview_window("youtube-stream-bridge") {
        window
    } else {
        let window = WebviewWindowBuilder::new(&app, "youtube-stream-bridge", WebviewUrl::App("index.html".into()))
            .title("VibeDeck - YouTube Music stream bridge")
            .visible(false)
            .initialization_script(r#"(() => { window.addEventListener('beforeunload', (event) => event.stopImmediatePropagation(), true); const silence = (media) => { if (!(media instanceof HTMLMediaElement)) return; media.muted = true; media.defaultMuted = true; media.volume = 0; }; const play = HTMLMediaElement.prototype.play; HTMLMediaElement.prototype.play = function(...args) { silence(this); return play.apply(this, args); }; new MutationObserver((records) => records.forEach((record) => record.addedNodes.forEach((node) => { if (node instanceof HTMLMediaElement) silence(node); if (node instanceof Element) node.querySelectorAll?.('audio, video').forEach(silence); }))).observe(document, { childList: true, subtree: true }); document.querySelectorAll?.('audio, video').forEach(silence); })();"#)
            .on_page_load(|window, payload| {
                write_native_log(window.app_handle(), &format!("YouTube stream bridge page: {:?} {}", payload.event(), payload.url()));
                if payload.url().as_str().contains("music.youtube.com/watch") {
                    let script = r#"(() => { let attempts = 0; let requested = false; const isAudio = (url) => /(?:mime=audio|mime=audio%2F|type=audio|audio_only)/i.test(url) && !/mime=video/i.test(url); const collect = () => { const resource = performance.getEntriesByType('resource').map((entry) => entry.name).reverse().find((url) => url.includes('googlevideo.com/videoplayback') && isAudio(url)); if (resource) { document.title = '__VIBEDECK_STREAM__' + encodeURIComponent(resource); return; } const video = document.querySelector('video'); if (video && !requested) { requested = true; video.muted = true; video.volume = 0; video.pause(); video.preload = 'auto'; video.load(); } if (attempts++ < 80) setTimeout(collect, 150); }; collect(); })();"#;
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
                }
            })
            .build()
            .map_err(|error| error.to_string())?;
        let session_path = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?
            .join("youtube-session.bin");
        let protected_session = std::fs::read(session_path).map_err(|_| {
            "Faça login no YouTube Music antes de reproduzir uma faixa.".to_string()
        })?;
        let cookie_header = String::from_utf8(protect_data(protected_session, true)?)
            .map_err(|error| error.to_string())?;
        for cookie in cookie_header
            .split(';')
            .filter_map(|item| item.trim().split_once('='))
        {
            let cookie =
                tauri::webview::Cookie::build((cookie.0.to_string(), cookie.1.to_string()))
                    .domain(".youtube.com")
                    .path("/")
                    .secure(true)
                    .build();
            window
                .set_cookie(cookie)
                .map_err(|error| error.to_string())?;
        }
        window
    };
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
    for _ in 0..100 {
        if let Some(url) = result.lock().map_err(|error| error.to_string())?.take() {
            write_native_log(
                &app,
                "YouTube Music broker resolved a googlevideo audio stream",
            );
            if let Err(error) = bridge.close() {
                write_native_log(
                    &app,
                    &format!("YouTube stream bridge close after resolution failed: {error}"),
                );
            } else {
                write_native_log(&app, "YouTube stream bridge closed after resolution");
            }
            return Ok(serde_json::json!({ "url": url, "mimeType": "audio/mp4" }));
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    let _ = bridge.close();
    write_native_log(
        &app,
        "YouTube stream bridge closed after resolution timeout",
    );
    Err("Tempo limite ao resolver o stream de áudio do YouTube Music.".to_string())
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
                                *text != " • "
                                    && *text != "Música"
                                    && *text != "Vídeo"
                                    && *text != "Álbum"
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
fn cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|cookie| {
        cookie
            .trim()
            .strip_prefix(&format!("{name}="))
            .map(str::to_string)
    })
}

#[cfg(windows)]
fn build_sapisid_authorization(
    _cookie_header: &str,
    timestamp: u64,
    sapisid: &str,
) -> Result<String, String> {
    let origin = "https://music.youtube.com";
    let digest = |value: &str| {
        let hash = Sha1::digest(format!("{timestamp} {value} {origin}").as_bytes());
        hash.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let hash = digest(sapisid);
    Ok(format!("SAPISIDHASH {timestamp}_{hash}_u SAPISID1PHASH {timestamp}_{hash}_u SAPISID3PHASH {timestamp}_{hash}_u"))
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
fn seek_audio(
    app: tauri::AppHandle,
    request: audio::SeekRequest,
) -> Result<audio::PlayerSnapshot, String> {
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
        .invoke_handler(tauri::generate_handler![
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
            search_youtube_music,
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
            local_library::load_local_library,
            local_library::import_local_paths,
            local_library::cancel_local_import,
            local_library::remove_local_track,
            playlist_catalog::load_playlist_catalog,
            playlist_catalog::save_youtube_playlist_page
        ])
        .run(tauri::generate_context!())
        .expect("error while running VibeDeck");
}

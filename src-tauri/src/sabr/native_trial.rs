//! Explicit opt-in vertical trial. No UI/startup caller, no search or mutation.
//! Persistent operation limits survive restarts; only numeric accounting persists.
use crate::audio::{AudioEngine, PlayerActivation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::Manager;
const BODY: usize = 4 * 1024 * 1024;
const MEDIA: usize = 32 * 1024 * 1024;
static BUSY: AtomicBool = AtomicBool::new(false);
struct Busy;
impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    attempts: u8,
    music: u8,
    script: u8,
    player: u8,
    sabr: u8,
    wire_bytes: usize,
    audio_reserved: bool,
    selection: Option<(usize, usize, u64)>,
}

fn update_ledger<T>(
    path: &Path,
    change: impl FnOnce(&mut Ledger) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    use std::io::{Read, Seek, Write};
    let (mut file, new) = match std::fs::OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(f) => (f, true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .map_err(|_| "trial_ledger_unavailable")?,
            false,
        ),
        Err(_) => return Err("trial_ledger_unavailable"),
    };
    file.try_lock().map_err(|_| "trial_ledger_busy")?;
    if file
        .metadata()
        .map_err(|_| "trial_ledger_unavailable")?
        .len()
        > 4096
    {
        return Err("trial_ledger_invalid");
    }
    let mut ledger = if new {
        Ledger::default()
    } else {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| "trial_ledger_unavailable")?;
        serde_json::from_slice(&bytes).map_err(|_| "trial_ledger_invalid")?
    };
    if ledger.attempts > 10
        || ledger.music > 10
        || ledger.script > 10
        || ledger.player > 10
        || ledger.sabr > 50
        || ledger.wire_bytes > MEDIA
    {
        return Err("trial_ledger_invalid");
    }
    let value = change(&mut ledger)?;
    let bytes = serde_json::to_vec(&ledger).map_err(|_| "trial_ledger_invalid")?;
    file.rewind()
        .and_then(|_| file.write_all(&bytes))
        .and_then(|_| file.set_len(bytes.len() as u64))
        .and_then(|_| file.sync_all())
        .map_err(|_| "trial_ledger_write_failed")?;
    Ok(value)
}

fn reserve_trial(path: &Path, kind: &str) -> Result<usize, &'static str> {
    update_ledger(path, |l| {
        let (count, limit) = match kind {
            "music" => (&mut l.music, 10),
            "script" => (&mut l.script, 10),
            "player" => (&mut l.player, 10),
            "sabr" => (&mut l.sabr, 50),
            _ => return Err("trial_budget_operation_rejected"),
        };
        if *count >= limit {
            return Err("trial_budget_exhausted");
        }
        let bytes = if kind == "sabr" {
            let bytes = BODY.min(MEDIA - l.wire_bytes);
            if bytes == 0 {
                return Err("trial_media_budget_exhausted");
            }
            l.wire_bytes += bytes;
            bytes
        } else {
            0
        };
        *count += 1;
        Ok(bytes)
    })
}

#[tauri::command]
pub(crate) async fn debug_native_playback_trial(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    attempt_ordinal: u8,
) -> Result<Value, String> {
    if window.label() != "main" {
        return Err("trial_wrong_window".into());
    }
    if BUSY.swap(true, Ordering::AcqRel) {
        return Err("trial_busy".into());
    }
    let _busy = Busy;
    if AudioEngine::is_active() {
        return Err("trial_existing_playback".into());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "trial_directory_failed")?;
    let ledger = dir.join("native-vertical-trial-20261022.count");
    update_ledger(&ledger, |l| {
        if l.audio_reserved
            || attempt_ordinal == 0
            || attempt_ordinal != l.attempts + 1
            || l.attempts >= 10
        {
            return Err("trial_attempt_rejected");
        }
        l.attempts += 1;
        Ok(())
    })?;
    let mut report = json!({"result":"trial_pending","attemptOrdinal":attempt_ordinal,"audioStarted":false,"stage":"local"});
    if let Err(code) = run_trial(&app, &dir, &ledger, &mut report).await {
        report["result"] = json!(code);
    }
    let counts = update_ledger(&ledger, |l| {
        Ok(
            json!({"music":l.music,"script":l.script,"player":l.player,"sabr":l.sabr,"wireBytes":l.wire_bytes,"audioReserved":l.audio_reserved}),
        )
    })?;
    report["consumption"] = counts;
    Ok(report)
}

async fn run_trial(
    app: &tauri::AppHandle,
    dir: &Path,
    ledger: &Path,
    report: &mut Value,
) -> Result<(), String> {
    use vibedeck_decipher_eval::native_transform::{
        inspect_native_script, invoke_native_transform,
    };
    let catalog = crate::playlist_catalog::load_playlist_catalog(app.clone())
        .map_err(|_| "trial_catalog_unavailable")?;
    let indices = update_ledger(ledger, |l| {
        if let Some(s) = l.selection {
            return Ok(s);
        }
        for (pi, p) in catalog.playlists.iter().enumerate() {
            for (ti, t) in p.tracks.iter().enumerate() {
                if t.get("source").and_then(Value::as_str) == Some("youtube")
                    && t.get("youtubeId")
                        .and_then(Value::as_str)
                        .is_some_and(|id| {
                            id.len() == 11
                                && id
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                        })
                {
                    let s = (pi, ti, p.generation);
                    l.selection = Some(s);
                    return Ok(s);
                }
            }
        }
        Err("trial_catalog_track_missing")
    })?;
    let playlist = catalog
        .playlists
        .get(indices.0)
        .filter(|p| p.generation == indices.2)
        .ok_or("trial_catalog_changed")?;
    let track = playlist
        .tracks
        .get(indices.1)
        .ok_or("trial_catalog_changed")?;
    let video = track
        .get("youtubeId")
        .and_then(Value::as_str)
        .ok_or("trial_catalog_track_missing")?;
    let session = crate::youtube_session::YouTubeSession::load(&dir.join("youtube-session.bin"))
        .map_err(|_| "trial_session_unavailable")?;
    let now = crate::unix_now().map_err(|_| "trial_clock_failed")?;
    if !session.has_auth_cookie(now) {
        return Err("trial_session_ineligible".into());
    }
    let cookie = session
        .cookie_header_for("https://music.youtube.com/", now)
        .map_err(|_| "trial_session_ineligible")?;
    let client = crate::youtube_http::client()?;
    report["stage"] = json!("bootstrap");
    reserve_trial(ledger, "music")?;
    let response = client
        .get("https://music.youtube.com/")
        .header("Cookie", cookie)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .send()
        .await
        .map_err(|e| crate::youtube_http::request_error(&e))?;
    let bytes = crate::youtube_http::bounded_body(response, BODY).await?;
    let html = std::str::from_utf8(&bytes).map_err(|_| "trial_html_encoding")?;
    let declaration = vibedeck_bootstrap_inspect::script_diagnostic::select_music_script(html)?;
    let script_url = format!("https://music.youtube.com{}", declaration.path());
    report["stage"] = json!("script");
    reserve_trial(ledger, "script")?;
    let response = client
        .get(script_url)
        .send()
        .await
        .map_err(|e| crate::youtube_http::request_error(&e))?;
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !matches!(
        mime.split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "application/javascript" | "text/javascript"
    ) {
        return Err("trial_script_mime").map_err(String::from);
    }
    let script_bytes = crate::youtube_http::bounded_body(response, BODY).await?;
    let source = std::str::from_utf8(&script_bytes).map_err(|_| "trial_script_encoding")?;
    report["scriptBytes"] = json!(source.len());
    let inspected = inspect_native_script(source)?;
    report["timestampProperties"] = json!(inspected.timestamp_properties);
    report["urlEntries"] = json!(inspected.url_entries);
    report["nInputs"] = json!(inspected.n_inputs);
    report["transformCalls"] = json!(inspected.transform_calls);
    report["recognitionCounts"] = json!(inspected.recognition_counts);
    report["nShapeCounts"] = json!(inspected.n_shape_counts);
    report["globalNCounts"] = json!(inspected.global_n_counts);
    report["extraction"] = json!(inspected
        .transform
        .as_ref()
        .map(|_| "ready")
        .unwrap_or_else(|e| *e));
    if let Err(code) = &inspected.transform {
        return Err((*code).into());
    }
    report["stage"] = json!("player");
    let (payload, version) =
        fetch_trial_player(&client, &session, html, video, inspected.timestamp, ledger).await?;
    report["playability"] = json!(crate::youtube_http::playability_status(&payload));
    let mut bootstrap = super::SabrBootstrap::from_player_payload(&payload, video, &version)
        .map_err(|_| "trial_player_bootstrap_rejected")?;
    report["stage"] = json!("url_transform");
    let mut transform_called = false;
    let url = super::url_resolution::transform_n(&bootstrap.streaming_url, |input| {
        transform_called = true;
        let fragment = inspected.transform.as_ref().map_err(|_| ())?;
        invoke_native_transform(fragment, input).map_err(|_| ())
    })
    .map_err(|_| "trial_url_transform_failed")?;
    report["transformExecuted"] = json!(transform_called);
    bootstrap.streaming_url = url.to_string();
    let activation:PlayerActivation=serde_json::from_value(json!({"track":track,"queue":[track],"playlistId":playlist.id,"queueSource":"youtube","queueGeneration":playlist.generation,"queueIndex":0,"repeatMode":"off","activationSequence":crate::unix_now().map_err(|_|"trial_clock_failed")?})).map_err(|_|"trial_activation_rejected")?;
    report["stage"] = json!("transport");
    run_transport(bootstrap, activation, ledger, report).await
}

async fn fetch_trial_player(
    client: &reqwest::Client,
    session: &crate::youtube_session::YouTubeSession,
    html: &str,
    video: &str,
    timestamp: u32,
    ledger: &Path,
) -> Result<(Value, String), String> {
    let read = |key: &str| -> String {
        regex::Regex::new(&format!(r#""{}":"([^"]+)""#, regex::escape(key)))
            .ok()
            .and_then(|p| {
                p.captures(html)
                    .and_then(|c| c.get(1).map(|v| v.as_str().to_string()))
            })
            .unwrap_or_default()
    };
    let visitor = read("VISITOR_DATA");
    let version = read("INNERTUBE_CLIENT_VERSION");
    let key = read("INNERTUBE_API_KEY");
    if visitor.is_empty() || version.is_empty() {
        return Err("trial_context_missing".into());
    }
    let endpoint = if key.is_empty() {
        "https://music.youtube.com/youtubei/v1/player?alt=json".to_string()
    } else {
        format!("https://music.youtube.com/youtubei/v1/player?alt=json&key={key}")
    };
    let body = json!({"context":{"client":{"clientName":"WEB_REMIX","clientVersion":version,"visitorData":visitor,"hl":read("HL"),"gl":read("GL"),"platform":"DESKTOP","clientFormFactor":"UNKNOWN_FORM_FACTOR"},"user":{"lockedSafetyMode":false},"request":{"useSsl":true}},"videoId":video,"contentCheckOk":true,"racyCheckOk":true,"playbackContext":{"contentPlaybackContext":{"signatureTimestamp":timestamp}}});
    let now = crate::unix_now().map_err(|_| "trial_clock_failed")?;
    let cookie = session
        .cookie_header_for(&endpoint, now)
        .map_err(|_| "trial_session_ineligible")?;
    let auth = session
        .authorization(&endpoint, now as u64, None)
        .map_err(|_| "trial_session_ineligible")?;
    reserve_trial(ledger, "player")?;
    let response = client
        .post(endpoint)
        .header("Cookie", cookie)
        .header("Authorization", auth)
        .header("X-Youtube-Client-Name", "67")
        .header("X-Youtube-Client-Version", &version)
        .header("X-Origin", "https://music.youtube.com")
        .header("Origin", "https://music.youtube.com")
        .header("X-Goog-AuthUser", "0")
        .header("X-Youtube-Bootstrap-Logged-In", "true")
        .header("X-Goog-Visitor-Id", visitor)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .json(&body)
        .send()
        .await
        .map_err(|e| crate::youtube_http::request_error(&e))?;
    let bytes = crate::youtube_http::bounded_body(response, BODY).await?;
    Ok((crate::youtube_http::player_json(&bytes)?, version))
}

fn transport_url(raw: &str, rn: usize) -> Result<url::Url, &'static str> {
    // Destination/envelopes were checked by transform_n, now preserve raw pairs.
    let mut url = super::url_resolution::transform_n(raw, |n| Ok(n.to_owned()))
        .map_err(|_| "trial_transport_url_rejected")?;
    let query = url.query().unwrap_or("");
    let mut pairs = Vec::new();
    for pair in query.split('&') {
        let (name, value) = url::form_urlencoded::parse(pair.as_bytes())
            .next()
            .unwrap_or_default();
        if matches!(name.as_ref(), "sparams" | "lsparams")
            && value.split(',').any(|v| matches!(v, "rn" | "alr"))
        {
            return Err("trial_signed_transport_query");
        }
        if name != "rn" && name != "alr" {
            pairs.push(pair);
        }
    }
    let mut query = pairs.join("&");
    if !query.is_empty() {
        query.push('&');
    }
    query.push_str(&format!("rn={rn}&alr=no"));
    url.set_query(Some(&query));
    Ok(url)
}

async fn run_transport(
    bootstrap: super::SabrBootstrap,
    activation: PlayerActivation,
    ledger: &Path,
    report: &mut Value,
) -> Result<(), String> {
    let mut state = super::AbrRequestState::new(0);
    let mut bodies = Vec::new();
    let mut total = 0;
    let client = crate::youtube_http::client()?;
    for rn in 0..5 {
        let url = transport_url(&bootstrap.streaming_url, rn)?;
        let request = super::build_abr_request(&bootstrap, &state);
        if request.len() > 256 * 1024 {
            return Err("trial_sabr_request_limit".into());
        }
        let allowance = reserve_trial(ledger, "sabr")?;
        let response = client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
            .header(reqwest::header::ACCEPT, "application/vnd.yt-ump")
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .body(request)
            .send()
            .await
            .map_err(|e| crate::youtube_http::request_error(&e))?;
        report["transportStatus"] = json!(response.status().as_u16());
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !mime
            .split(';')
            .next()
            .is_some_and(|s| s.trim().eq_ignore_ascii_case("application/vnd.yt-ump"))
        {
            return Err("trial_sabr_mime".into());
        }
        let body = crate::youtube_http::bounded_body(response, allowance).await?;
        update_ledger(ledger, |l| {
            l.wire_bytes -= allowance - body.len();
            Ok(())
        })?;
        let control = super::parse_response_parts(&body).map_err(|_| "trial_ump_rejected")?;
        report["mediaBytes"] = json!(control.media_bytes);
        report["segments"] = json!(control.segments.len());
        report["protectionPresent"] = json!(control.stream_protection_status.is_some());
        if control.redirect_url.is_some()
            || control.reload_token.is_some()
            || control.seek_to_ms.is_some()
            || control.error_code.is_some()
        {
            return Err("trial_ump_control_requires_handling".into());
        }
        total += control.media_bytes;
        bodies.push(body);
        if total > 0
            && control
                .segments
                .iter()
                .any(|s| !s.is_initialization && s.media_bytes > 0)
        {
            break;
        }
        let mut changed = false;
        if let Some(policy) = control.next_request_policy {
            if let Some(cookie) = policy.playback_cookie {
                changed |= state.playback_cookie.as_ref() != Some(&cookie);
                state.playback_cookie = Some(cookie);
            }
            if let Some(backoff) = policy.backoff_time_ms {
                if backoff > 5000 {
                    return Err("trial_sabr_backoff".into());
                }
                if backoff > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(backoff as u64)).await;
                }
            }
        }
        for update in control.context_updates {
            if update.send_by_default {
                state.sabr_contexts.retain(|(k, _)| *k != update.kind);
                state.sabr_contexts.push((update.kind, update.value));
                changed = true;
            }
        }
        if !changed {
            return Err("trial_sabr_no_progress".into());
        }
    }
    if total == 0 {
        return Err("trial_sabr_no_media".into());
    }
    report["stage"] = json!("audio");
    update_ledger(ledger, |l| {
        if l.audio_reserved {
            return Err("trial_audio_budget_exhausted");
        }
        l.audio_reserved = true;
        Ok(())
    })?;
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    std::thread::spawn(move || {
        for _ in 0..300 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if flag.load(Ordering::Acquire) {
                return;
            }
        }
        AudioEngine::stop();
    });
    struct StopAudio(std::sync::Arc<AtomicBool>);
    impl Drop for StopAudio {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
            AudioEngine::stop();
        }
    }
    let _stop = StopAudio(cancelled);
    let (tx, rx) = std::sync::mpsc::channel();
    for body in bodies {
        tx.send(Ok(body)).map_err(|_| "trial_audio_channel")?;
    }
    drop(tx);
    let started = tauri::async_runtime::spawn_blocking(move || {
        AudioEngine::play_ump_receiver(rx, activation)
    })
    .await
    .map_err(|_| "trial_audio_worker")?;
    started.map_err(|_| "trial_audio_start_failed")?;
    report["audioStarted"] = json!(true);
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    let diagnostic = AudioEngine::diagnostics_snapshot();
    report["decoderStarted"] = json!(diagnostic.decoder_started);
    report["outputStarted"] = json!(diagnostic.output_started);
    report["positionMs"] = json!((AudioEngine::position() * 1000.0).max(0.0) as u64);
    report["result"] = json!("trial_output_observed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ledger_enforces_limits_without_reset() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("trial-ledger-{}-{nonce}", std::process::id()));
        for _ in 0..10 {
            assert!(reserve_trial(&path, "music").is_ok());
        }
        assert_eq!(reserve_trial(&path, "music"), Err("trial_budget_exhausted"));
        for _ in 0..8 {
            assert_eq!(reserve_trial(&path, "sabr"), Ok(BODY));
        }
        assert_eq!(
            reserve_trial(&path, "sabr"),
            Err("trial_media_budget_exhausted")
        );
        std::fs::write(&path, b"invalid").unwrap();
        assert!(reserve_trial(&path, "script").is_err());
        std::fs::remove_file(path).unwrap();
    }
}

//! Native byte-range playback session: header + `sidx`, paced chunks, pause/seek/renewal/end, driven by
//! a background task while the audio engine plays the UMP bodies it receives. Needs no `pot` and sends no
//! cookies on media requests; see `native_range` for how the signed URL is produced.

use crate::audio::{AudioEngine, PlayerActivation, SeekRequest};
use crate::native_range::{resolve_audio, NativeMediaReport, Resolved};
use crate::sabr::range_plan::{parse_sidx, plan_ranges, segment_at, ump_media_bytes, SegmentIndex};
use crate::sabr::range_policy::{decide, secs_to_expiry, Action, Inputs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const CHUNK_BYTES: u64 = 192 * 1024;
const AHEAD_MS: u64 = 30_000;
const MIN_GAP: Duration = Duration::from_millis(350);
const TICK: Duration = Duration::from_millis(250);
/// How long a seek waits for further targets (slider drag) before the stream is restarted.
const SEEK_SETTLE: Duration = Duration::from_millis(300);
const BODY_LIMIT: usize = 2 * 1024 * 1024;
const MAX_CONSECUTIVE_FAILURES: u8 = 3;
const MAX_RENEWAL_FAILURES: u8 = 2;

type Body = Result<Vec<u8>, String>;

enum Command {
    /// Target in seconds, plus where to report when the stream was restarted (or why not).
    Seek(f64, mpsc::Sender<Result<(), String>>),
}

struct Handle {
    cancel: Arc<AtomicBool>,
    commands: mpsc::Sender<Command>,
    activation_sequence: u64,
}

static SESSION: Mutex<Option<Handle>> = Mutex::new(None);

/// One byte-range POST with the 2-byte body; only 2xx with a UMP content type succeeds.
pub async fn fetch_body(http: &reqwest::Client, url: reqwest::Url) -> Result<Vec<u8>, String> {
    let response = http
        .post(url)
        .header("Origin", "https://music.youtube.com")
        .header("Referer", "https://music.youtube.com/")
        .header("Accept", "*/*")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        )
        .body(vec![0x78u8, 0x00])
        .send()
        .await
        .map_err(|e| crate::youtube_http::request_error(&e).to_string())?;
    let status = response.status();
    if matches!(status.as_u16(), 401 | 403) {
        return Err("native_range_access_denied".into());
    }
    if !status.is_success() {
        return Err("native_range_http_status".into());
    }
    let ump = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/vnd.yt-ump"));
    if !ump {
        return Err("native_range_not_ump".into());
    }
    crate::youtube_http::bounded_body(response, BODY_LIMIT).await
}

struct Feeder {
    video: String,
    resolved: Resolved,
    http: reqwest::Client,
    rn: u32,
    last: Instant,
    denied: bool,
}

impl Feeder {
    async fn get(&mut self, range: (u64, u64), rbuf: u32) -> Result<Vec<u8>, String> {
        let wait = MIN_GAP.saturating_sub(self.last.elapsed());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        self.rn += 1;
        let url = self.resolved.url(range, self.rn, rbuf)?;
        let body = fetch_body(&self.http, url).await;
        self.last = Instant::now();
        body
    }
}

struct Stream {
    tx: Option<mpsc::Sender<Body>>,
    plan: Vec<(u64, u64)>,
    next: usize,
    /// Media time (ms) covered by what was requested so far.
    sent_ms: u64,
    /// Media time (ms) that the engine's position 0 corresponds to.
    base_ms: u64,
}

fn end_time(index: &SegmentIndex, byte_end: u64) -> u64 {
    index
        .segments
        .iter()
        .rposition(|s| s.start <= byte_end)
        .map_or(0, |i| index.segments[i].start_ms + index.segments[i].dur_ms)
}

fn engine_position_ms() -> u64 {
    (AudioEngine::position() * 1000.0).max(0.0) as u64
}

fn range_u64(format: &serde_json::Value, key: &str, field: &str) -> Option<u64> {
    format.get(key)?.get(field)?.as_str()?.parse().ok()
}

fn current_sequence() -> u64 {
    AudioEngine::snapshot().activation_sequence
}

/// Starts native playback of `video_id`. Returns once the engine is playing; a background task keeps
/// feeding it. Any failure before that point is returned so the caller can fall back to another path.
pub async fn play(
    app: &tauri::AppHandle,
    video_id: &str,
    activation: PlayerActivation,
) -> Result<serde_json::Value, String> {
    cancel();
    let sequence = activation.activation_sequence;
    let mut report = NativeMediaReport::default();
    let resolved = resolve_audio(app, video_id, &mut report).await?;
    let init_end = range_u64(&resolved.format, "initRange", "end").ok_or("native_range_init_missing")?;
    let index_start =
        range_u64(&resolved.format, "indexRange", "start").ok_or("native_range_index_missing")?;
    let index_end =
        range_u64(&resolved.format, "indexRange", "end").ok_or("native_range_index_missing")?;
    if index_start <= init_end || index_end <= index_start || index_end > 4 * 1024 * 1024 {
        return Err("native_range_ranges_rejected".into());
    }
    let mut feeder = Feeder {
        video: video_id.to_string(),
        resolved,
        http: crate::youtube_http::client()?,
        rn: 0,
        last: Instant::now() - MIN_GAP,
        denied: false,
    };
    let header = feeder.get((0, index_end), 0).await?;
    let media = ump_media_bytes(&header);
    if media.len() as u64 != index_end + 1 {
        return Err("native_range_header_incomplete".into());
    }
    let mut index = parse_sidx(&media[index_start as usize..], index_start)?;
    index.header_end = index_end;

    let (tx, rx) = mpsc::channel::<Body>();
    tx.send(Ok(header.clone())).map_err(|_| "native_range_channel_closed")?;
    let mut stream = Stream {
        tx: Some(tx),
        plan: plan_ranges(&index, 0, false, CHUNK_BYTES, 4000),
        next: 0,
        sent_ms: 0,
        base_ms: 0,
    };
    prefetch(&mut feeder, &mut stream, &index, 2).await?;
    let engine_activation = activation.clone();
    tauri::async_runtime::spawn_blocking(move || {
        AudioEngine::play_ump_receiver(rx, engine_activation)
    })
    .await
    .map_err(|_| "native_range_engine_join_failed".to_string())??;

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let (commands, command_rx) = mpsc::channel::<Command>();
    if let Ok(mut slot) = SESSION.lock() {
        *slot = Some(Handle {
            cancel: cancel_flag.clone(),
            commands,
            activation_sequence: sequence,
        });
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        drive(app, feeder, index, header, stream, sequence, cancel_flag, command_rx).await;
    });
    Ok(serde_json::json!({
        "scope": "native_byte_range",
        "audioEngineStarted": true,
        "continuation": true,
        "seek": true
    }))
}

async fn prefetch(
    feeder: &mut Feeder,
    stream: &mut Stream,
    index: &SegmentIndex,
    count: usize,
) -> Result<(), String> {
    for _ in 0..count {
        let Some(&(a, b)) = stream.plan.get(stream.next) else { break };
        let body = feeder.get((a, b), 0).await?;
        stream
            .tx
            .as_ref()
            .ok_or("native_range_channel_closed")?
            .send(Ok(body))
            .map_err(|_| "native_range_channel_closed")?;
        stream.sent_ms = end_time(index, b);
        stream.next += 1;
    }
    Ok(())
}

/// Stops the running session, if any. The engine is stopped by the caller.
pub fn cancel() {
    if let Ok(mut slot) = SESSION.lock() {
        if let Some(handle) = slot.take() {
            handle.cancel.store(true, Ordering::Release);
        }
    }
}

/// Routes a UI seek to the running native session and waits (bounded) until the stream was
/// restarted, so the snapshot the UI gets back already carries the new revision. `Ok(false)` means
/// there is no session (the caller keeps its own path); the engine validates track, queue
/// generation and revision.
pub async fn seek(request: &SeekRequest) -> Result<bool, String> {
    let sequence = current_sequence();
    let (ack_tx, ack_rx) = mpsc::channel::<Result<(), String>>();
    {
        let slot = SESSION.lock().map_err(|_| "native_range_state_poisoned")?;
        let Some(handle) = slot.as_ref() else { return Ok(false) };
        if handle.activation_sequence != sequence || handle.cancel.load(Ordering::Acquire) {
            return Ok(false);
        }
        AudioEngine::validate_seek(request)?;
        handle
            .commands
            .send(Command::Seek(request.seconds, ack_tx))
            .map_err(|_| "native_range_session_gone".to_string())?;
    }
    tauri::async_runtime::spawn_blocking(move || ack_rx.recv_timeout(Duration::from_secs(12)))
        .await
        .map_err(|_| "native_range_seek_join_failed".to_string())?
        .map_err(|_| "native_range_seek_timeout".to_string())??;
    Ok(true)
}

fn finish_session(sequence: u64) {
    if let Ok(mut slot) = SESSION.lock() {
        if slot.as_ref().is_some_and(|h| h.activation_sequence == sequence) {
            *slot = None;
        }
    }
}

/// Tells the engine the stream failed (it reports the track as errored) and closes the channel.
fn fail(stream: &mut Stream, code: &str) {
    if let Some(tx) = stream.tx.take() {
        let _ = tx.send(Err(code.to_string()));
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive(
    app: tauri::AppHandle,
    mut feeder: Feeder,
    index: SegmentIndex,
    header: Vec<u8>,
    mut stream: Stream,
    sequence: u64,
    cancel: Arc<AtomicBool>,
    commands: mpsc::Receiver<Command>,
) {
    let (mut failures, mut renewal_failures) = (0u8, 0u8);
    loop {
        if cancel.load(Ordering::Acquire) || current_sequence() != sequence {
            break;
        }
        if !AudioEngine::is_active() {
            break;
        }
        if let Ok(Command::Seek(first, first_ack)) = commands.try_recv() {
            // Dragging the slider sends many targets in a row: wait until they settle and restart
            // the stream once, at the last one. Earlier requests are answered together with it.
            let (mut seconds, mut acks) = (first, vec![first_ack]);
            let settle = Instant::now();
            loop {
                tokio::time::sleep(SEEK_SETTLE).await;
                let mut more = false;
                while let Ok(Command::Seek(next, ack)) = commands.try_recv() {
                    seconds = next;
                    acks.push(ack);
                    more = true;
                }
                if !more || settle.elapsed() > Duration::from_secs(2) {
                    break;
                }
            }
            eprintln!("VibeDeck native range seek: target={seconds:.1}s merged_requests={}", acks.len());
            match do_seek(&mut feeder, &index, &header, seconds, sequence).await {
                Ok(new_stream) => {
                    stream = new_stream;
                    failures = 0;
                    for ack in acks {
                        let _ = ack.send(Ok(()));
                    }
                    crate::emit_player_event(&app, "stateChanged");
                }
                Err(code) => {
                    eprintln!("VibeDeck native range seek failed: {code}");
                    for ack in acks {
                        let _ = ack.send(Err(code.clone()));
                    }
                    fail(&mut stream, "native_range_seek_failed");
                    break;
                }
            }
            continue;
        }
        let now = crate::unix_now().unwrap_or(0);
        let inputs = Inputs {
            paused: AudioEngine::is_paused(),
            buffered_until_ms: stream.sent_ms,
            position_ms: stream.base_ms + engine_position_ms(),
            ahead_ms: AHEAD_MS,
            plan_left: stream.next < stream.plan.len(),
            secs_to_expiry: secs_to_expiry(&feeder.resolved.parts.url, now),
            last_denied: feeder.denied,
            force_renew: false,
        };
        match decide(&inputs) {
            Action::Wait => tokio::time::sleep(TICK).await,
            Action::Finish => {
                // Closing the channel lets the engine end the track by itself.
                stream.tx = None;
                while AudioEngine::is_active()
                    && !cancel.load(Ordering::Acquire)
                    && current_sequence() == sequence
                    && commands.try_recv().is_err()
                {
                    tokio::time::sleep(TICK).await;
                }
                break;
            }
            Action::Renew => {
                match resolve_audio(&app, &feeder.video, &mut NativeMediaReport::default()).await {
                    Ok(fresh) => {
                        feeder.resolved = fresh;
                        feeder.denied = false;
                        renewal_failures = 0;
                    }
                    Err(code) => {
                        renewal_failures += 1;
                        eprintln!("VibeDeck native range renewal failed: {code}");
                        if renewal_failures >= MAX_RENEWAL_FAILURES {
                            fail(&mut stream, "native_range_renewal_failed");
                            break;
                        }
                        feeder.denied = false;
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
            Action::Fetch => {
                let (a, b) = stream.plan[stream.next];
                let rbuf = stream
                    .sent_ms
                    .saturating_sub(inputs.position_ms)
                    .min(u64::from(u32::MAX)) as u32;
                match feeder.get((a, b), rbuf).await {
                    Ok(body) => {
                        failures = 0;
                        feeder.denied = false;
                        if let Some(tx) = &stream.tx {
                            if tx.send(Ok(body)).is_err() {
                                break;
                            }
                        }
                        stream.sent_ms = end_time(&index, b);
                        stream.next += 1;
                    }
                    Err(code) if code == "native_range_access_denied" => feeder.denied = true,
                    Err(code) => {
                        failures += 1;
                        eprintln!("VibeDeck native range request failed: {code}");
                        if failures >= MAX_CONSECUTIVE_FAILURES {
                            fail(&mut stream, "native_range_stalled");
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
    finish_session(sequence);
}

async fn do_seek(
    feeder: &mut Feeder,
    index: &SegmentIndex,
    header: &[u8],
    seconds: f64,
    sequence: u64,
) -> Result<Stream, String> {
    let duration_ms = index.segments.last().map_or(0, |s| s.start_ms + s.dur_ms);
    let target_ms = ((seconds.max(0.0) * 1000.0) as u64).min(duration_ms.saturating_sub(1));
    let segment = segment_at(index, target_ms);
    let (tx, rx) = mpsc::channel::<Body>();
    tx.send(Ok(header.to_vec())).map_err(|_| "native_range_channel_closed")?;
    let start_ms = index.segments[segment].start_ms;
    let mut stream = Stream {
        tx: Some(tx),
        plan: plan_ranges(index, segment, false, CHUNK_BYTES, 4000),
        next: 0,
        sent_ms: start_ms,
        // The engine position becomes the absolute media time of the wanted point.
        base_ms: 0,
    };
    prefetch(feeder, &mut stream, index, 1).await?;
    if current_sequence() != sequence {
        return Err("native_range_superseded".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        AudioEngine::restart_ump_receiver(
            rx,
            target_ms as f64 / 1000.0,
            (target_ms - start_ms) as f64 / 1000.0,
        )
    })
    .await
    .map_err(|_| "native_range_engine_join_failed".to_string())??;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_time_maps_a_byte_to_the_end_of_its_segment() {
        let index = SegmentIndex {
            segments: vec![
                crate::sabr::range_plan::Segment { start: 100, size: 100, start_ms: 0, dur_ms: 5000 },
                crate::sabr::range_plan::Segment { start: 200, size: 100, start_ms: 5000, dur_ms: 4000 },
            ],
            header_end: 99,
            total: 300,
        };
        assert_eq!(end_time(&index, 150), 5000);
        assert_eq!(end_time(&index, 200), 9000);
        assert_eq!(end_time(&index, 299), 9000);
    }

    #[test]
    fn range_values_are_read_as_strings_from_the_format() {
        let format = serde_json::json!({"initRange": {"start": "0", "end": "631"}});
        assert_eq!(range_u64(&format, "initRange", "end"), Some(631));
        assert_eq!(range_u64(&format, "indexRange", "end"), None);
    }

    #[test]
    fn seek_without_a_session_is_not_handled() {
        cancel();
        let request = SeekRequest {
            track_id: "x".into(),
            queue_generation: 1,
            seconds: 10.0,
            revision: 1,
        };
        let result = tauri::async_runtime::block_on(seek(&request));
        assert_eq!(result, Ok(false));
    }
}

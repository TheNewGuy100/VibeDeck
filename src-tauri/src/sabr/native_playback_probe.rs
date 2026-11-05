//! Explicit debug IPC only; needs the `native-playback-trial` feature. One attempt per process.
//! Plays the first catalog track through the app's audio engine using only native requests in the
//! byte-range UMP mode (no `pot`, no cookies on media). Phases: header + `sidx`; normal playback with
//! paced chunks; optional pause (no requests while paused); a seek that restarts the stream from the
//! target segment; optional forced URL renewal; optional run to the end of the track. AUDIBLE.
//! The request budget is exact and enforced here. Reports counts and positions only.

use crate::native_range::{resolve_audio, NativeMediaReport, Resolved};
use super::range_plan::{parse_sidx, plan_ranges, segment_at, ump_media_bytes, SegmentIndex};
use super::range_policy::{decide, secs_to_expiry, Action, Inputs};
use super::range_probe::fetch_body;
use crate::audio::{AudioEngine, PlayerActivation};
use std::sync::mpsc;
use std::time::{Duration, Instant};

static ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const CHUNK_BYTES: u64 = 192 * 1024;
const AHEAD_MS: u64 = 30_000;
const MIN_GAP: Duration = Duration::from_millis(350);
const TICK: Duration = Duration::from_millis(500);

#[derive(serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Budget {
    /// Ceiling: 3 for the base resolution, plus 2 for each renewal.
    auxiliary_get: u8,
    /// 1, plus 1 when a renewal is requested.
    player_post: u8,
    media_posts: u8,
    /// Seconds of normal playback before the next phase (5 to 40).
    play_seconds: u8,
    /// Seek target as a percentage of the duration (5 to 98).
    seek_percent: u8,
    /// 0 skips the pause phase; otherwise 3 to 15 seconds paused.
    pause_seconds: u8,
    /// 0 never renews; otherwise renew once after this many media requests (2 to media_posts).
    renew_after_posts: u8,
    /// Keep serving until the track is exhausted and wait for the engine to end by itself.
    finish: bool,
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackReport {
    pub resolve: NativeMediaReport,
    pub stage: &'static str,
    pub error_code: Option<String>,
    pub media_posts_used: u8,
    pub header_bytes: usize,
    pub segments: usize,
    pub duration_ms: u64,
    pub chunks_before_seek: u8,
    pub chunks_after_seek: u8,
    /// `[ms since start, engine position in ms, engine active]` samples.
    pub samples: Vec<(u64, u64, bool)>,
    pub seek_target_ms: Option<u64>,
    pub seek_segment: Option<usize>,
    pub paused_seconds: u8,
    pub posts_during_pause: Option<u8>,
    pub pause_position_frozen: Option<bool>,
    pub resume_position_advanced: Option<bool>,
    pub renewals: u8,
    pub renew_error: Option<String>,
    pub denied_responses: u8,
    pub stream_closed: bool,
    pub ended_cleanly: Option<bool>,
    pub end_wait_ms: Option<u64>,
    pub engine_active_at_end: bool,
}

struct Feeder {
    video: String,
    resolved: Resolved,
    http: reqwest::Client,
    left: u8,
    used: u8,
    rn: u32,
    last: Instant,
    last_denied: bool,
}

impl Feeder {
    async fn get(&mut self, range: (u64, u64), rbuf: u32) -> Result<Vec<u8>, String> {
        if self.left == 0 {
            return Err("playback_budget_exhausted".into());
        }
        let wait = MIN_GAP.saturating_sub(self.last.elapsed());
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        self.left -= 1;
        self.used += 1;
        self.rn += 1;
        let url = self.resolved.url(range, self.rn, rbuf)?;
        let body = fetch_body(&self.http, url).await.map_err(String::from);
        self.last = Instant::now();
        body
    }
}

/// One live stream feeding the engine: its channel, the remaining plan and what was already requested.
struct Stream {
    tx: Option<mpsc::Sender<Result<Vec<u8>, String>>>,
    plan: Vec<(u64, u64)>,
    next: usize,
    /// Media time (ms) covered by what has been requested so far.
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

fn position_ms() -> u64 {
    (AudioEngine::position() * 1000.0).max(0.0) as u64
}

fn sample(report: &mut PlaybackReport, started: Instant) {
    report.samples.push((started.elapsed().as_millis() as u64, position_ms(), AudioEngine::is_active()));
}

fn valid(budget: &Budget) -> bool {
    let renews = u8::from(budget.renew_after_posts > 0);
    budget.auxiliary_get == 3 + 2 * renews
        && budget.player_post == 1 + renews
        && (4..=40).contains(&budget.media_posts)
        && (5..=40).contains(&budget.play_seconds)
        && (5..=98).contains(&budget.seek_percent)
        && (budget.pause_seconds == 0 || (3..=15).contains(&budget.pause_seconds))
        && (budget.renew_after_posts == 0
            || (2..=budget.media_posts).contains(&budget.renew_after_posts))
}

fn range_u64(format: &serde_json::Value, key: &str, field: &str) -> Option<u64> {
    format.get(key)?.get(field)?.as_str()?.parse().ok()
}

fn activation(app: &tauri::AppHandle) -> Result<PlayerActivation, String> {
    let catalog = crate::playlist_catalog::load_playlist_catalog(app.clone())
        .map_err(|_| "playback_catalog_unavailable")?;
    for playlist in &catalog.playlists {
        for track in &playlist.tracks {
            let good = track.get("source").and_then(|v| v.as_str()) == Some("youtube")
                && track.get("youtubeId").and_then(|v| v.as_str()).is_some_and(|id| {
                    id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                });
            if good {
                return serde_json::from_value(serde_json::json!({
                    "track": track, "queue": [track], "playlistId": playlist.id,
                    "queueSource": "youtube", "queueGeneration": playlist.generation,
                    "queueIndex": 0, "repeatMode": "off",
                    "activationSequence": crate::unix_now().map_err(|_| "playback_clock_failed")?
                }))
                .map_err(|_| "playback_activation_rejected".to_string());
            }
        }
    }
    Err("playback_catalog_track_missing".into())
}

async fn start_engine(
    rx: mpsc::Receiver<Result<Vec<u8>, String>>,
    activation: PlayerActivation,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || AudioEngine::play_ump_receiver(rx, activation))
        .await
        .map_err(|_| "playback_engine_join_failed".to_string())?
}

/// One scheduler step: asks the policy what to do and does it.
async fn tick(
    app: &tauri::AppHandle,
    budget: &Budget,
    feeder: &mut Feeder,
    stream: &mut Stream,
    index: &SegmentIndex,
    report: &mut PlaybackReport,
) -> Result<(), String> {
    let now = crate::unix_now().map_err(|_| "playback_clock_failed")?;
    let position = stream.base_ms + position_ms();
    let force_renew = budget.renew_after_posts > 0
        && feeder.used >= budget.renew_after_posts
        && report.renewals == 0
        && report.renew_error.is_none();
    let inputs = Inputs {
        paused: AudioEngine::is_paused(),
        buffered_until_ms: stream.sent_ms,
        position_ms: position,
        ahead_ms: AHEAD_MS,
        plan_left: stream.next < stream.plan.len(),
        secs_to_expiry: secs_to_expiry(&feeder.resolved.parts.url, now),
        last_denied: feeder.last_denied,
        force_renew,
    };
    match decide(&inputs) {
        Action::Wait => tokio::time::sleep(TICK).await,
        Action::Finish => {
            if stream.tx.take().is_some() {
                report.stream_closed = true;
            }
            tokio::time::sleep(TICK).await;
        }
        Action::Renew => {
            // A renewal that fails must not loop: the error is recorded and requests continue with the old URL.
            match resolve_audio(app, &feeder.video, &mut NativeMediaReport::default()).await {
                Ok(fresh) => {
                    feeder.resolved = fresh;
                    feeder.last_denied = false;
                    report.renewals += 1;
                }
                Err(code) => {
                    report.renew_error = Some(code);
                    feeder.last_denied = false;
                }
            }
        }
        Action::Fetch => {
            let (a, b) = stream.plan[stream.next];
            let rbuf = stream.sent_ms.saturating_sub(position).min(u64::from(u32::MAX)) as u32;
            match feeder.get((a, b), rbuf).await {
                Ok(body) => {
                    if let Some(tx) = &stream.tx {
                        tx.send(Ok(body)).map_err(|_| "playback_channel_closed")?;
                    }
                    stream.sent_ms = end_time(index, b);
                    stream.next += 1;
                    feeder.last_denied = false;
                }
                Err(code) if code.contains("access_denied") => {
                    report.denied_responses += 1;
                    feeder.last_denied = true;
                }
                Err(code) => return Err(code),
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn debug_youtube_native_playback_once(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: Budget,
) -> Result<PlaybackReport, String> {
    if window.label() != "main" {
        return Err("playback_wrong_window".into());
    }
    if !valid(&budget) {
        return Err("playback_budget_rejected".into());
    }
    if ATTEMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("playback_already_attempted".into());
    }
    if AudioEngine::is_active() {
        return Err("playback_engine_busy".into());
    }
    let mut report = PlaybackReport { stage: "start", ..Default::default() };
    if let Err(code) = run(&app, &budget, &mut report).await {
        report.error_code = Some(code);
    }
    AudioEngine::set_paused(false);
    AudioEngine::stop();
    report.engine_active_at_end = AudioEngine::is_active();
    Ok(report)
}

async fn run(app: &tauri::AppHandle, budget: &Budget, report: &mut PlaybackReport) -> Result<(), String> {
    let activation = activation(app)?;
    let video = super::client_survey::first_catalog_video(app)?;
    let resolved = resolve_audio(app, &video, &mut report.resolve).await?;
    let init_end = range_u64(&resolved.format, "initRange", "end").ok_or("playback_init_range_missing")?;
    let (index_start, index_end) = (
        range_u64(&resolved.format, "indexRange", "start").ok_or("playback_index_range_missing")?,
        range_u64(&resolved.format, "indexRange", "end").ok_or("playback_index_range_missing")?,
    );
    if index_start <= init_end || index_end <= index_start || index_end > 4 * 1024 * 1024 {
        return Err("playback_ranges_rejected".into());
    }
    let mut feeder = Feeder {
        video,
        resolved,
        http: crate::youtube_http::client()?,
        left: budget.media_posts,
        used: 0,
        rn: 0,
        last: Instant::now() - MIN_GAP,
        last_denied: false,
    };
    // 1. Header: initialization + index, parsed here for the time-to-byte map.
    report.stage = "header";
    let header = feeder.get((0, index_end), 0).await?;
    report.header_bytes = header.len();
    let media = ump_media_bytes(&header);
    if media.len() as u64 != index_end + 1 {
        return Err("playback_header_incomplete".into());
    }
    let mut index: SegmentIndex = parse_sidx(&media[index_start as usize..], index_start)?;
    index.header_end = index_end;
    report.segments = index.segments.len();
    report.duration_ms = index.segments.last().map_or(0, |s| s.start_ms + s.dur_ms);

    // 2. Normal playback from the start.
    report.stage = "play";
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    tx.send(Ok(header.clone())).map_err(|_| "playback_channel_closed")?;
    let mut stream = Stream {
        tx: Some(tx),
        plan: plan_ranges(&index, 0, false, CHUNK_BYTES, 4000),
        next: 0,
        sent_ms: 0,
        base_ms: 0,
    };
    for _ in 0..2 {
        let Some(&(a, b)) = stream.plan.get(stream.next) else { break };
        let body = feeder.get((a, b), 0).await?;
        stream.tx.as_ref().ok_or("playback_channel_closed")?.send(Ok(body)).map_err(|_| "playback_channel_closed")?;
        stream.sent_ms = end_time(&index, b);
        stream.next += 1;
        report.chunks_before_seek += 1;
    }
    start_engine(rx, activation.clone()).await?;
    let started = Instant::now();
    sample(report, started);
    let before = stream.next;
    let phase = Instant::now();
    while phase.elapsed() < Duration::from_secs(u64::from(budget.play_seconds)) {
        tick(app, budget, &mut feeder, &mut stream, &index, report).await?;
        sample(report, started);
    }
    report.chunks_before_seek += (stream.next - before) as u8;

    // 3. Optional pause: the policy must stop requesting and the position must freeze.
    if budget.pause_seconds > 0 {
        report.stage = "pause";
        report.paused_seconds = budget.pause_seconds;
        let (used_before, pos_before) = (feeder.used, position_ms());
        AudioEngine::set_paused(true);
        tokio::time::sleep(Duration::from_millis(600)).await;
        let frozen_from = position_ms();
        let phase = Instant::now();
        while phase.elapsed() < Duration::from_secs(u64::from(budget.pause_seconds)) {
            tick(app, budget, &mut feeder, &mut stream, &index, report).await?;
            sample(report, started);
        }
        let frozen_to = position_ms();
        report.posts_during_pause = Some(feeder.used - used_before);
        report.pause_position_frozen = Some(frozen_to.saturating_sub(frozen_from) < 400 && frozen_from >= pos_before);
        AudioEngine::set_paused(false);
        let resumed_from = position_ms();
        let phase = Instant::now();
        while phase.elapsed() < Duration::from_secs(4) {
            tick(app, budget, &mut feeder, &mut stream, &index, report).await?;
            sample(report, started);
        }
        report.resume_position_advanced = Some(position_ms().saturating_sub(resumed_from) > 2000);
    }

    // 4. Seek: restart the stream from the header plus the target segment.
    report.stage = "seek";
    let target_ms = report.duration_ms * u64::from(budget.seek_percent) / 100;
    let segment = segment_at(&index, target_ms);
    report.seek_target_ms = Some(target_ms);
    report.seek_segment = Some(segment);
    stream.tx = None;
    let (tx2, rx2) = mpsc::channel::<Result<Vec<u8>, String>>();
    tx2.send(Ok(header)).map_err(|_| "playback_channel_closed")?;
    let mut stream = Stream {
        tx: Some(tx2),
        plan: plan_ranges(&index, segment, false, CHUNK_BYTES, 4000),
        next: 0,
        sent_ms: index.segments[segment].start_ms,
        base_ms: index.segments[segment].start_ms,
    };
    for _ in 0..2 {
        if feeder.left == 0 {
            break;
        }
        let Some(&(a, b)) = stream.plan.get(stream.next) else { break };
        let body = feeder.get((a, b), 0).await?;
        stream.tx.as_ref().ok_or("playback_channel_closed")?.send(Ok(body)).map_err(|_| "playback_channel_closed")?;
        stream.sent_ms = end_time(&index, b);
        stream.next += 1;
        report.chunks_after_seek += 1;
    }
    // The engine refuses an activation that is not newer than the current one.
    let mut restart = activation;
    restart.activation_sequence = restart.activation_sequence.saturating_add(1);
    start_engine(rx2, restart).await?;
    report.stage = "after_seek";
    sample(report, started);
    let before = stream.next;
    let phase = Instant::now();
    while phase.elapsed() < Duration::from_secs(8) {
        tick(app, budget, &mut feeder, &mut stream, &index, report).await?;
        sample(report, started);
    }

    // 5. Optional: serve what is left, close the stream and let the engine end by itself.
    if budget.finish {
        report.stage = "finish";
        let limit = Instant::now();
        while stream.tx.is_some() && limit.elapsed() < Duration::from_secs(120) {
            tick(app, budget, &mut feeder, &mut stream, &index, report).await?;
            sample(report, started);
        }
        let waited = Instant::now();
        while AudioEngine::is_active() && waited.elapsed() < Duration::from_secs(45) {
            tokio::time::sleep(TICK).await;
            sample(report, started);
        }
        report.end_wait_ms = Some(waited.elapsed().as_millis() as u64);
        report.ended_cleanly = Some(report.stream_closed && !AudioEngine::is_active());
    }
    report.chunks_after_seek += (stream.next - before) as u8;
    report.media_posts_used = feeder.used;
    report.stage = "done";
    Ok(())
}

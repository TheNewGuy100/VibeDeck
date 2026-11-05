//! Explicit debug IPC only; no automatic invocation.
//! A bounded SABR session: a few POSTs guided by the previous response (playback cookie, contexts,
//! buffered ranges, redirect), sent in the web player format with the `n` solved and no PO token.
//! The question it answers: does the server start asking for the token after the first request?
//! The report holds counts, classes and timings only; no URL, cookie, context or token value.

use super::request_shape::{describe_request, describe_url, RequestShape, UrlShape};
use super::transport_probe::{
    content_type_class, load_bootstrap, mirror_url_with, prepare_url_with, replace_n,
    solve_n_blocking, status_class, status_error, RequestBudget,
};
use super::{
    build_abr_request, parse_response_parts, AbrRequestState, AudioFormat, BufferedRange,
    SabrBootstrap, SabrControl,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MIN_ROUNDS: u8 = 2;
const MAX_ROUNDS: u8 = 8;
const BODY_LIMIT: usize = 4 * 1024 * 1024;
const REQUEST_LIMIT: usize = 256 * 1024;
const BACKOFF_CAP_MS: i32 = 5000;

static ATTEMPTED: AtomicBool = AtomicBool::new(false);

fn check_request(
    budget: &RequestBudget,
    rounds: u8,
    seek_percent: Option<u8>,
) -> Result<(), &'static str> {
    if !(MIN_ROUNDS..=MAX_ROUNDS).contains(&rounds)
        || budget.sabr_posts() != Some(rounds)
        || seek_percent.is_some_and(|percent| !(1..=95).contains(&percent))
    {
        return Err("sabr_session_budget_rejected");
    }
    Ok(())
}

/// No call from UI/startup. The budget must be exactly 1 GET, 1 player POST and `rounds` SABR POSTs.
/// One session per process, including failures. `seek_percent` makes the last round a seek.
#[tauri::command]
pub async fn debug_youtube_sabr_session_once(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: RequestBudget,
    rounds: u8,
    seek_percent: Option<u8>,
) -> Result<SessionReport, String> {
    if window.label() != "main" {
        return Err("sabr_probe_wrong_window".into());
    }
    check_request(&budget, rounds, seek_percent)?;
    if ATTEMPTED.swap(true, Ordering::SeqCst) {
        return Err("sabr_probe_already_attempted".into());
    }
    let bootstrap = load_bootstrap(&app).await?;
    Ok(run_session(bootstrap, rounds, seek_percent).await)
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundReport {
    pub round: u8,
    pub seek: bool,
    pub player_time_ms: i64,
    pub buffered_ranges_sent: usize,
    pub initialized_formats_sent: usize,
    pub cookie_sent: bool,
    pub contexts_sent: usize,
    pub status_class: Option<&'static str>,
    pub content_type_class: Option<&'static str>,
    pub error_code: Option<&'static str>,
    pub response_bytes: usize,
    pub media_bytes: usize,
    pub media_segments: usize,
    pub init_segments: usize,
    pub first_start_ms: Option<i64>,
    pub last_end_ms: Option<i64>,
    pub protection_status: Option<i32>,
    pub protection_max_retries: Option<i32>,
    pub policy_present: bool,
    pub cookie_received: bool,
    pub backoff_ms: Option<i32>,
    pub target_audio_readahead_ms: Option<i32>,
    pub min_audio_readahead_ms: Option<i32>,
    pub max_time_since_last_request_ms: Option<i32>,
    pub context_updates: usize,
    pub contexts_started: usize,
    pub contexts_stopped: usize,
    pub contexts_discarded: usize,
    pub redirect: bool,
    pub reload: bool,
    pub server_seek_ms: Option<i64>,
    pub sabr_error: Option<i32>,
    pub end_of_track: bool,
    /// `(part type, count)`; the type numbers are protocol constants, not data.
    pub part_counts: Vec<(u32, u32)>,
    /// Names and length classes of what was actually sent in this round.
    pub request_shape: Option<RequestShape>,
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReport {
    pub rounds_planned: u8,
    pub rounds_sent: u8,
    pub n_solved: bool,
    pub redirects_followed: u8,
    pub stop_reason: Option<&'static str>,
    pub error_code: Option<&'static str>,
    pub url_shape: Option<UrlShape>,
    pub rounds: Vec<RoundReport>,
}

pub async fn run_session(
    bootstrap: SabrBootstrap,
    rounds: u8,
    seek_percent: Option<u8>,
) -> SessionReport {
    let mut report = SessionReport {
        rounds_planned: rounds,
        ..Default::default()
    };
    if let Err(code) = drive(bootstrap, rounds, seek_percent, &mut report).await {
        report.error_code = Some(code);
    }
    report
}

/// Solves `n` (once per distinct challenge), then applies the web player URL format.
async fn resolve_url(
    raw: &str,
    client_version: &str,
    cpn: &mut Option<String>,
    cache: &mut Option<(String, String)>,
    report: &mut SessionReport,
) -> Result<reqwest::Url, &'static str> {
    let parsed = reqwest::Url::parse(raw).map_err(|_| "sabr_probe_invalid_url")?;
    let challenge = parsed
        .query_pairs()
        .find(|(name, _)| name == "n")
        .map(|(_, value)| value.into_owned());
    let mut raw_url = raw.to_string();
    if let Some(challenge) = challenge {
        let already_solved = cache
            .as_ref()
            .is_some_and(|(_, solved)| *solved == challenge);
        if !already_solved {
            let solved = match cache {
                Some((known, solved)) if *known == challenge => solved.clone(),
                _ => {
                    let input = challenge.clone();
                    let solved =
                        tauri::async_runtime::spawn_blocking(move || solve_n_blocking(&input))
                            .await
                            .map_err(|_| "sabr_probe_solver_failed")??;
                    *cache = Some((challenge, solved.clone()));
                    solved
                }
            };
            raw_url = replace_n(raw, &solved).ok_or("sabr_probe_n_missing")?;
            report.n_solved = true;
        }
    }
    let mut url = prepare_url_with(&raw_url, true)?;
    *cpn = Some(mirror_url_with(&mut url, client_version, cpn.as_deref(), 0));
    Ok(url)
}

async fn drive(
    mut bootstrap: SabrBootstrap,
    rounds: u8,
    seek_percent: Option<u8>,
    report: &mut SessionReport,
) -> Result<(), &'static str> {
    // The web player gets its token from a client-side minter; this session sends none.
    bootstrap.po_token = None;
    if let Ok(parsed) = reqwest::Url::parse(&bootstrap.streaming_url) {
        report.url_shape = Some(describe_url(&parsed));
    }
    let client_version = bootstrap.client_version.clone();
    let mut cpn: Option<String> = None;
    let mut solved_cache: Option<(String, String)> = None;
    let mut url = resolve_url(
        &bootstrap.streaming_url,
        &client_version,
        &mut cpn,
        &mut solved_cache,
        report,
    )
    .await?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "sabr_probe_client_failed")?;
    let audio = bootstrap.selected_audio().clone();
    let duration_ms = if bootstrap.duration_ms > 0 {
        bootstrap.duration_ms
    } else {
        i64::try_from(audio.approx_duration_ms).unwrap_or_default()
    };

    let started = Instant::now();
    let mut last_seek = started;
    let mut last_request: Option<Instant> = None;
    let mut playing_since: Option<Instant> = None;
    let mut position_base = 0i64;
    let mut idle_rounds = 0u8;
    let mut state = AbrRequestState::new(0);
    state.browser_shape = true;

    for round in 0..rounds {
        let seek = seek_percent.is_some() && round == rounds - 1;
        if let Some(percent) = seek_percent.filter(|_| seek) {
            position_base = duration_ms * i64::from(percent) / 100;
            playing_since = Some(Instant::now());
            state.buffered_ranges.clear();
            last_seek = Instant::now();
        }
        let mut position = playing_since.map_or(position_base, |since| {
            position_base + i64::try_from(since.elapsed().as_millis()).unwrap_or(i64::MAX)
        });
        if let Some(end) = state
            .buffered_ranges
            .iter()
            .map(|range| range.start_time_ms + range.duration_ms)
            .max()
        {
            position = position.min(end);
        }
        let millis =
            |since: Instant| i64::try_from(since.elapsed().as_millis()).unwrap_or(i64::MAX);
        state.player_time_ms = position;
        // The first request carries zero timers, exactly like the single-request probe did.
        let first = last_request.is_none();
        state.elapsed_wall_time_ms = if first { 0 } else { millis(started) };
        state.time_since_last_seek_ms = if first { 0 } else { millis(last_seek) };
        state.time_since_last_action_ms = last_request.map_or(0, millis);

        mirror_url_with(&mut url, &client_version, cpn.as_deref(), u32::from(round));
        let request = build_abr_request(&bootstrap, &state);
        if request.len() > REQUEST_LIMIT {
            return Err("sabr_probe_request_too_large");
        }
        let mut entry = RoundReport {
            request_shape: Some(describe_request(&url, &[], &request)),
            round,
            seek,
            player_time_ms: position,
            buffered_ranges_sent: state.buffered_ranges.len(),
            initialized_formats_sent: state.initialized_formats.len(),
            cookie_sent: state.playback_cookie.is_some(),
            contexts_sent: state.sabr_contexts.len(),
            ..Default::default()
        };
        report.rounds_sent += 1;
        last_request = Some(Instant::now());
        let outcome = exchange(&client, &url, request, &mut entry).await;
        let control = match outcome {
            Ok(control) => control,
            Err(code) => {
                entry.error_code = Some(code);
                report.rounds.push(entry);
                return Err(code);
            }
        };
        fill_entry(&mut entry, &control);
        let progressed = advance_state(&mut state, &control, &audio);
        if control.media_bytes > 0 && playing_since.is_none() {
            playing_since = Some(Instant::now());
        }
        let stop = if control.reload_token.is_some() {
            Some("reload_requested")
        } else if control.error_code.is_some() {
            Some("sabr_error_part")
        } else if control.end_of_track {
            Some("end_of_track")
        } else {
            None
        };
        let backoff = control
            .next_request_policy
            .as_ref()
            .and_then(|policy| policy.backoff_time_ms)
            .unwrap_or_default()
            .clamp(0, BACKOFF_CAP_MS);
        let redirect = control.redirect_url.clone();
        report.rounds.push(entry);
        if let Some(reason) = stop {
            report.stop_reason = Some(reason);
            return Ok(());
        }
        if let Some(target) = redirect {
            url = resolve_url(
                &target,
                &client_version,
                &mut cpn,
                &mut solved_cache,
                report,
            )
            .await?;
            report.redirects_followed += 1;
        } else if progressed {
            idle_rounds = 0;
        } else {
            idle_rounds += 1;
            if idle_rounds >= 2 {
                report.stop_reason = Some("no_progress");
                return Ok(());
            }
        }
        if backoff > 0 && round + 1 < rounds {
            tokio::time::sleep(Duration::from_millis(backoff as u64)).await;
        }
    }
    report.stop_reason = Some("rounds_done");
    Ok(())
}

async fn exchange(
    client: &reqwest::Client,
    url: &reqwest::Url,
    request: Vec<u8>,
    entry: &mut RoundReport,
) -> Result<SabrControl, &'static str> {
    let response = client
        .post(url.clone())
        .body(request)
        .send()
        .await
        .map_err(|error| super::super::youtube_http::request_error(&error))?;
    entry.status_class = Some(status_class(response.status()));
    entry.content_type_class = Some(content_type_class(response.headers()));
    if !response.status().is_success() {
        return Err(status_error(response.status()));
    }
    if entry.content_type_class != Some("ump") {
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
    entry.response_bytes = body.len();
    parse_response_parts(&body).map_err(|_| "sabr_session_ump_rejected")
}

fn fill_entry(entry: &mut RoundReport, control: &SabrControl) {
    entry.media_bytes = control.media_bytes;
    entry.media_segments = control
        .segments
        .iter()
        .filter(|segment| !segment.is_initialization)
        .count();
    entry.init_segments = control
        .segments
        .iter()
        .filter(|segment| segment.is_initialization)
        .count();
    entry.first_start_ms = control
        .segments
        .iter()
        .filter(|segment| !segment.is_initialization)
        .map(|segment| segment.start_ms)
        .min();
    entry.last_end_ms = control.buffered_until_ms();
    entry.protection_status = control.stream_protection_status;
    entry.protection_max_retries = control.stream_protection_max_retries;
    if let Some(policy) = &control.next_request_policy {
        entry.policy_present = true;
        entry.cookie_received = policy.playback_cookie.is_some();
        entry.backoff_ms = policy.backoff_time_ms;
        entry.target_audio_readahead_ms = policy.target_audio_readahead_ms;
        entry.min_audio_readahead_ms = policy.min_audio_readahead_ms;
        entry.max_time_since_last_request_ms = policy.max_time_since_last_request_ms;
    }
    entry.context_updates = control.context_updates.len();
    entry.contexts_started = control.start_contexts.len();
    entry.contexts_stopped = control.stop_contexts.len();
    entry.contexts_discarded = control.discard_contexts.len();
    entry.redirect = control.redirect_url.is_some();
    entry.reload = control.reload_token.is_some();
    entry.server_seek_ms = control.seek_to_ms;
    entry.sabr_error = control.error_code;
    entry.end_of_track = control.end_of_track;
    entry.part_counts = control.part_counts.clone();
}

/// Folds one response into the next request. Returns true when something new arrived (media, a
/// changed cookie or a changed context), which is what separates "progress" from "idle".
fn advance_state(state: &mut AbrRequestState, control: &SabrControl, audio: &AudioFormat) -> bool {
    let mut progressed = false;
    let mut audio_seen = false;
    for segment in control.segments.iter().filter(|s| s.itag == audio.id.itag) {
        audio_seen = true;
        if segment.is_initialization || segment.media_bytes == 0 {
            continue;
        }
        progressed = true;
        match state.buffered_ranges.last_mut() {
            Some(last) if segment.segment_num <= last.end_segment_index => {}
            Some(last) if segment.segment_num == last.end_segment_index + 1 => {
                last.duration_ms = (segment.start_ms + segment.duration_ms) - last.start_time_ms;
                last.end_segment_index = segment.segment_num;
            }
            _ => state.buffered_ranges.push(BufferedRange {
                format: audio.id.clone(),
                start_time_ms: segment.start_ms,
                duration_ms: segment.duration_ms,
                start_segment_index: segment.segment_num,
                end_segment_index: segment.segment_num,
            }),
        }
    }
    if audio_seen && !state.initialized_formats.contains(&audio.id) {
        state.initialized_formats.push(audio.id.clone());
    }
    if let Some(cookie) = control
        .next_request_policy
        .as_ref()
        .and_then(|policy| policy.playback_cookie.clone())
    {
        progressed |= state.playback_cookie.as_ref() != Some(&cookie);
        state.playback_cookie = Some(cookie);
    }
    for update in &control.context_updates {
        if update.send_by_default {
            let existing = state
                .sabr_contexts
                .iter()
                .find(|(kind, _)| *kind == update.kind);
            progressed |= existing.map(|(_, value)| value) != Some(&update.value);
            state.sabr_contexts.retain(|(kind, _)| *kind != update.kind);
            state
                .sabr_contexts
                .push((update.kind, update.value.clone()));
        }
    }
    for kind in control
        .stop_contexts
        .iter()
        .chain(&control.discard_contexts)
    {
        state.sabr_contexts.retain(|(known, _)| known != kind);
    }
    progressed
}

#[cfg(test)]
mod tests {
    use super::super::{FormatId, NextRequestPolicy, SabrContextUpdate, SegmentInfo};
    use super::*;

    fn audio() -> AudioFormat {
        AudioFormat {
            id: FormatId {
                itag: 251,
                last_modified: 7,
                xtags: String::new(),
            },
            mime_type: "audio/webm".into(),
            bitrate: 1,
            audio_quality: String::new(),
            sample_rate: 48_000,
            channels: 2,
            approx_duration_ms: 0,
            content_length: 0,
            init_range: None,
        }
    }

    fn segment(num: i32, start_ms: i64, init: bool, bytes: usize) -> SegmentInfo {
        SegmentInfo {
            itag: 251,
            segment_num: num,
            start_ms,
            duration_ms: 5000,
            is_initialization: init,
            media_bytes: bytes,
            ..Default::default()
        }
    }

    #[test]
    fn budget_must_match_rounds_and_seek_must_be_inside_the_track() {
        let budget = |sabr: u8| -> RequestBudget {
            serde_json::from_value(serde_json::json!({
                "auxiliaryGet": 1, "playerPost": 1, "sabrPost": sabr
            }))
            .unwrap()
        };
        assert!(check_request(&budget(4), 4, None).is_ok());
        assert!(check_request(&budget(4), 4, Some(50)).is_ok());
        for (sabr, rounds, seek) in [
            (3, 4, None),
            (1, 1, None),
            (9, 9, None),
            (4, 4, Some(0)),
            (4, 4, Some(96)),
        ] {
            assert_eq!(
                check_request(&budget(sabr), rounds, seek),
                Err("sabr_session_budget_rejected"),
                "{sabr} {rounds} {seek:?}"
            );
        }
        let extra: Result<RequestBudget, _> = serde_json::from_value(serde_json::json!({
            "auxiliaryGet": 2, "playerPost": 1, "sabrPost": 4
        }));
        assert_eq!(
            check_request(&extra.unwrap(), 4, None),
            Err("sabr_session_budget_rejected")
        );
    }

    #[test]
    fn response_state_feeds_the_next_request() {
        let audio = audio();
        let mut state = AbrRequestState::new(0);
        let control = SabrControl {
            media_bytes: 10,
            segments: vec![
                segment(0, 0, true, 0),
                segment(1, 0, false, 5),
                segment(2, 5000, false, 5),
            ],
            next_request_policy: Some(NextRequestPolicy {
                playback_cookie: Some(vec![1, 2]),
                ..Default::default()
            }),
            context_updates: vec![SabrContextUpdate {
                kind: 3,
                value: vec![9],
                send_by_default: true,
                keep_existing: false,
            }],
            ..Default::default()
        };
        assert!(advance_state(&mut state, &control, &audio));
        assert_eq!(state.initialized_formats, vec![audio.id.clone()]);
        assert_eq!(state.buffered_ranges.len(), 1);
        assert_eq!(state.buffered_ranges[0].start_segment_index, 1);
        assert_eq!(state.buffered_ranges[0].end_segment_index, 2);
        assert_eq!(state.buffered_ranges[0].duration_ms, 10_000);
        assert_eq!(state.playback_cookie, Some(vec![1, 2]));
        assert_eq!(state.sabr_contexts, vec![(3, vec![9])]);

        // The same cookie and context with no media is idle; the init id is not duplicated.
        let repeat = SabrControl {
            next_request_policy: control.next_request_policy.clone(),
            context_updates: control.context_updates.clone(),
            segments: vec![segment(0, 0, true, 0)],
            ..Default::default()
        };
        assert!(!advance_state(&mut state, &repeat, &audio));
        assert_eq!(state.initialized_formats.len(), 1);

        // A gap starts a new range; a stop/discard removes the context.
        let gap = SabrControl {
            segments: vec![segment(6, 25_000, false, 5)],
            stop_contexts: vec![3],
            ..Default::default()
        };
        assert!(advance_state(&mut state, &gap, &audio));
        assert_eq!(state.buffered_ranges.len(), 2);
        assert!(state.sabr_contexts.is_empty());
    }
}

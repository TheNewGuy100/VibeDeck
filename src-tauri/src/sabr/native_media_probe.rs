//! Explicit debug IPC only; needs the `native-playback-trial` feature. One attempt per process.
//! End to end, all native: `/player` for the first catalog track, then `n` and `s` answered by the
//! yt-dlp-ejs solver from a per-revision prepared player, then ONE byte-range POST without `pot`
//! and without cookies. Reports durations, lengths and response classes, never URLs or values.

use super::range_probe::{send, RangeReport};
use crate::native_range::{resolve_audio, NativeMediaReport};

static ATTEMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Budget {
    auxiliary_get: u8,
    player_post: u8,
    media_post: u8,
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaProbeReport {
    #[serde(flatten)]
    pub resolve: NativeMediaReport,
    pub media: Option<RangeReport>,
}

#[tauri::command]
pub async fn debug_youtube_native_media_once(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: Budget,
) -> Result<MediaProbeReport, String> {
    if window.label() != "main" {
        return Err("native_media_wrong_window".into());
    }
    if budget != (Budget { auxiliary_get: 3, player_post: 1, media_post: 1 }) {
        return Err("native_media_budget_rejected".into());
    }
    if ATTEMPTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("native_media_already_attempted".into());
    }
    let mut report = MediaProbeReport::default();
    report.resolve.stage = "start";
    if let Err(code) = run(&app, &mut report).await {
        report.resolve.error_code = Some(code);
    }
    Ok(report)
}

async fn run(app: &tauri::AppHandle, report: &mut MediaProbeReport) -> Result<(), String> {
    let video = super::client_survey::first_catalog_video(app)?;
    let resolved = resolve_audio(app, &video, &mut report.resolve).await?;
    // 4. One byte-range POST without `pot`.
    report.resolve.stage = "media";
    let url = resolved.url((0, 65535), 1, 0)?;
    report.media = Some(send(url, false).await?);
    report.resolve.stage = "done";
    Ok(())
}

//! Explicit main-window IPC. No audio, script execution, dump or retries.
use super::player_script::{inspect_script_once, InspectionBudget};
use std::sync::atomic::{AtomicBool, Ordering};

static ATTEMPTED: AtomicBool = AtomicBool::new(false);

#[derive(serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScriptProbeBudget {
    auxiliary_get: u8,
    player_post: u8,
    script_get: u8,
}

fn reserve_script_probe(
    attempted: &AtomicBool,
    budget: &ScriptProbeBudget,
) -> Result<(), &'static str> {
    if *budget
        != (ScriptProbeBudget {
            auxiliary_get: 1,
            player_post: 1,
            script_get: 1,
        })
    {
        return Err("script_probe_budget_rejected");
    }
    attempted
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "script_probe_already_attempted")?;
    Ok(())
}

#[tauri::command]
pub(crate) async fn debug_youtube_player_script_once(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: ScriptProbeBudget,
) -> Result<serde_json::Value, String> {
    if window.label() != "main" {
        return Err("script_probe_wrong_window".into());
    }
    reserve_script_probe(&ATTEMPTED, &budget)?;
    let catalog = crate::playlist_catalog::load_playlist_catalog(app.clone())
        .map_err(|_| "script_probe_catalog_unavailable")?;
    let video_id = catalog
        .playlists
        .iter()
        .flat_map(|p| p.tracks.iter())
        .filter(|t| t.get("source").and_then(|v| v.as_str()) == Some("youtube"))
        .filter_map(|t| t.get("youtubeId").and_then(|v| v.as_str()))
        .find(|id| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        })
        .ok_or("script_probe_catalog_track_missing")?;
    let fetch = crate::fetch_youtube_player(&app, video_id)
        .await
        .map_err(|_| "script_probe_bootstrap_failed")?;
    let payload = &fetch.payload;
    let status = crate::youtube_http::playability_status(payload);
    if status != "OK" {
        return Ok(
            serde_json::json!({"playabilityStatus":status,"result":"script_probe_player_rejected","scriptBytes":0}),
        );
    }
    match inspect_script_once(payload, InspectionBudget { script_get: 1 }).await {
        Ok(script) => Ok(serde_json::json!({
            "playabilityStatus":status,
            "result":"script_probe_acquired",
            "boundToPayload":script.belongs_to(payload),
            "scriptBytes":script.source().len(),
        })),
        // Acquisition returns only fixed codes, never provider text/source/URL.
        Err(code) => {
            Ok(serde_json::json!({"playabilityStatus":status,"result":code,"scriptBytes":0}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_budget_and_single_reservation() {
        let attempted = AtomicBool::new(false);
        for raw in [
            r#"{}"#,
            r#"{"auxiliaryGet":1,"playerPost":1,"scriptGet":1,"sabrPost":1}"#,
        ] {
            assert!(serde_json::from_str::<ScriptProbeBudget>(raw).is_err());
        }
        let mut budget = ScriptProbeBudget {
            auxiliary_get: 1,
            player_post: 1,
            script_get: 0,
        };
        assert_eq!(
            reserve_script_probe(&attempted, &budget),
            Err("script_probe_budget_rejected")
        );
        budget.script_get = 1;
        assert_eq!(reserve_script_probe(&attempted, &budget), Ok(()));
        assert_eq!(
            reserve_script_probe(&attempted, &budget),
            Err("script_probe_already_attempted")
        );
    }
}

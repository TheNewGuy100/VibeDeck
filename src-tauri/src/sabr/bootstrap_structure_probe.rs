//! Separate checkpoint from historical lexical probes. One explicit GET, no retries.
use std::sync::atomic::{AtomicBool, Ordering};
const BODY_LIMIT: usize = 4 * 1024 * 1024;
static ATTEMPTED: AtomicBool = AtomicBool::new(false);

#[derive(serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StructuralBudget {
    auxiliary_get: u8,
}

fn reserve(attempted: &AtomicBool, budget: &StructuralBudget) -> Result<(), &'static str> {
    if budget.auxiliary_get != 1 {
        return Err("bootstrap_structure_budget_rejected");
    }
    attempted
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "bootstrap_structure_already_attempted")?;
    Ok(())
}

#[cfg(feature = "bootstrap-inspection")]
fn summarize(source: &str) -> Result<serde_json::Value, &'static str> {
    use vibedeck_bootstrap_inspect::bootstrap_html::{inspect_bootstrap_html, HtmlInspectionError};
    let report = inspect_bootstrap_html(source).map_err(|e| match e {
        HtmlInspectionError::SourceLimit => "bootstrap_structure_body_limit",
        HtmlInspectionError::HtmlParse => "bootstrap_structure_html_parse_rejected",
        HtmlInspectionError::NodeLimit => "bootstrap_structure_node_limit",
        HtmlInspectionError::ScriptSourceLimit => "bootstrap_structure_script_source_limit",
        HtmlInspectionError::ScriptCountLimit => "bootstrap_structure_script_count_limit",
    })?;
    // Syntax only: uniqueness never authorizes selection, download or execution.
    Ok(serde_json::json!({
        "result":"bootstrap_structure_inspected", "coverage":"html_ast_syntax_only",
        "bodyBytes":source.len(), "inlineScripts":report.inline_classic_scripts,
        "externalScripts":report.external_scripts, "unsupportedScripts":report.unsupported_scripts,
        "excludedSubtrees":report.excluded_subtrees,"javascriptParseFailures":report.javascript_parse_failures,
        "staticProperties":report.static_js_url_properties,"literalStrings":report.literal_string_properties,
        "otherValues":report.other_values,"accessorsOrMethods":report.accessors_or_methods,
        "computedLiteralKeys":report.computed_literal_keys,"scriptsWithProperties":report.scripts_with_static_properties,
        "directConfigProperties":report.direct_config_properties,"musicWatchProperties":report.music_watch_properties,
        "canonicalStringPaths":report.canonical_string_paths,"unsupportedConfigMembers":report.unsupported_config_members,
        "oversizedScripts":report.oversized_scripts,
        "oversizedScriptsWithLexicalKey":report.oversized_scripts_with_lexical_key,
        "largestInlineScriptBytes":report.largest_inline_script_bytes,
        "scriptCountLimited":report.script_count_limited,
        "partial":report.oversized_scripts > 0 || report.script_count_limited || report.javascript_parse_failures > 0,
        "ambiguous":report.static_js_url_properties > 1,
        "bindingValidated":false
    }))
}

#[cfg(not(feature = "bootstrap-inspection"))]
fn summarize(_: &str) -> Result<serde_json::Value, &'static str> {
    Err("bootstrap_structure_feature_required")
}

#[tauri::command]
pub(crate) async fn debug_youtube_bootstrap_structure_once(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: StructuralBudget,
) -> Result<serde_json::Value, String> {
    if window.label() != "main" {
        return Err("bootstrap_structure_wrong_window".into());
    }
    if !cfg!(feature = "bootstrap-inspection") {
        return Err("bootstrap_structure_feature_required".into());
    }
    reserve(&ATTEMPTED, &budget)?;
    perform_structural_get(app).await
}

async fn perform_structural_get(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| "bootstrap_structure_directory_failed")?
        .join("youtube-session.bin");
    let session = crate::youtube_session::YouTubeSession::load(&path)
        .map_err(|_| "bootstrap_structure_session_unavailable")?;
    let now = crate::unix_now().map_err(|_| "bootstrap_structure_clock_failed")?;
    if !session.has_auth_cookie(now) {
        return Err("bootstrap_structure_session_ineligible".into());
    }
    let cookie = session
        .cookie_header_for("https://music.youtube.com/", now)
        .map_err(|_| "bootstrap_structure_session_ineligible")?;
    let response = crate::youtube_http::client()?
        .get("https://music.youtube.com/")
        .header("Cookie", cookie)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .send()
        .await
        .map_err(|e| crate::youtube_http::request_error(&e))?;
    crate::youtube_http::successful_status(response.status())?;
    let html = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|m| m.trim().eq_ignore_ascii_case("text/html"))
        });
    if !html {
        return Err("bootstrap_structure_mime_rejected".into());
    }
    let bytes = crate::youtube_http::bounded_body(response, BODY_LIMIT).await?;
    let source = std::str::from_utf8(&bytes).map_err(|_| "bootstrap_structure_utf8_rejected")?;
    summarize(source).map_err(String::from)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CountedBudget {
    auxiliary_get: u8,
    request_ordinal: u8,
}

/// New explicitly approved allocation, NOT a reset of any historical one-shot guard.
/// Append-only numeric ledger is reserved/fsynced before session access. File lock
/// serializes processes; ordinal rejects replay. No account/context data is stored.
fn reserve_counted(path: &std::path::Path, budget: &CountedBudget) -> Result<u8, &'static str> {
    use std::io::{Read, Write};
    if budget.auxiliary_get != 1 || !(1..=20).contains(&budget.request_ordinal) {
        return Err("bootstrap_structure_budget_rejected");
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)
        .map_err(|_| "bootstrap_structure_ledger_unavailable")?;
    file.try_lock()
        .map_err(|_| "bootstrap_structure_ledger_busy")?;
    if file
        .metadata()
        .map_err(|_| "bootstrap_structure_ledger_unavailable")?
        .len()
        > 20
    {
        return Err("bootstrap_structure_ledger_invalid");
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(21)
        .read_to_end(&mut bytes)
        .map_err(|_| "bootstrap_structure_ledger_unavailable")?;
    if bytes.iter().any(|b| *b != b'\n') {
        return Err("bootstrap_structure_ledger_invalid");
    }
    if bytes.len() >= 20 {
        return Err("bootstrap_structure_budget_exhausted");
    }
    if budget.request_ordinal as usize != bytes.len() + 1 {
        return Err("bootstrap_structure_ordinal_rejected");
    }
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .map_err(|_| "bootstrap_structure_ledger_write_failed")?;
    Ok(budget.request_ordinal)
}

#[tauri::command]
pub(crate) async fn debug_youtube_bootstrap_structure_counted(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: CountedBudget,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    if window.label() != "main" {
        return Err("bootstrap_structure_wrong_window".into());
    }
    if !cfg!(feature = "bootstrap-inspection") {
        return Err("bootstrap_structure_feature_required".into());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "bootstrap_structure_directory_failed")?;
    let ordinal = reserve_counted(
        &dir.join("bootstrap-structure-expanded20-20261022.count"),
        &budget,
    )?;
    match perform_structural_get(app).await {
        Ok(mut result) => {
            result["requestOrdinal"] = serde_json::json!(ordinal);
            Ok(result)
        }
        Err(code) => Ok(serde_json::json!({"result":code,"requestOrdinal":ordinal})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counted_ledger_survives_reopen_rejects_replay_and_stops_at_twenty() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vibedeck-structure-test-{}-{nonce}.count",
            std::process::id()
        ));
        assert_eq!(
            reserve_counted(
                &path,
                &CountedBudget {
                    auxiliary_get: 1,
                    request_ordinal: 0
                }
            ),
            Err("bootstrap_structure_budget_rejected")
        );
        assert!(!path.exists());
        for ordinal in 1..=20 {
            assert_eq!(
                reserve_counted(
                    &path,
                    &CountedBudget {
                        auxiliary_get: 1,
                        request_ordinal: ordinal
                    }
                ),
                Ok(ordinal)
            );
            if ordinal < 20 {
                assert_eq!(
                    reserve_counted(
                        &path,
                        &CountedBudget {
                            auxiliary_get: 1,
                            request_ordinal: ordinal
                        }
                    ),
                    Err("bootstrap_structure_ordinal_rejected")
                );
            }
        }
        assert_eq!(
            reserve_counted(
                &path,
                &CountedBudget {
                    auxiliary_get: 1,
                    request_ordinal: 20
                }
            ),
            Err("bootstrap_structure_budget_exhausted")
        );
        assert_eq!(std::fs::read(&path).unwrap(), vec![b'\n'; 20]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn counted_ledger_rejects_corruption_without_rewriting_it() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vibedeck-structure-corrupt-{}-{nonce}.count",
            std::process::id()
        ));
        std::fs::write(&path, b"invalid").unwrap();
        assert_eq!(
            reserve_counted(
                &path,
                &CountedBudget {
                    auxiliary_get: 1,
                    request_ordinal: 1
                }
            ),
            Err("bootstrap_structure_ledger_invalid")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn exact_budget_is_single_use_including_failed_attempts() {
        let flag = AtomicBool::new(false);
        assert!(
            serde_json::from_str::<StructuralBudget>(r#"{"auxiliaryGet":1,"scriptGet":1}"#)
                .is_err()
        );
        assert_eq!(
            reserve(&flag, &StructuralBudget { auxiliary_get: 3 }),
            Err("bootstrap_structure_budget_rejected")
        );
        assert!(reserve(&flag, &StructuralBudget { auxiliary_get: 1 }).is_ok());
        assert_eq!(
            reserve(&flag, &StructuralBudget { auxiliary_get: 1 }),
            Err("bootstrap_structure_already_attempted")
        );
    }
    #[cfg(feature = "bootstrap-inspection")]
    #[test]
    fn structural_projection_exports_no_values_and_never_claims_binding() {
        let source = "<!doctype html><html><head><script>ytcfg.set({WEB_PLAYER_CONTEXT_CONFIGS:{WEB_PLAYER_CONTEXT_CONFIG_ID_MUSIC_WATCH:{jsUrl:'/s/player/synthetic-private/x/base.js'}}});</script></head><body></body></html>";
        let result = summarize(source).unwrap();
        assert_eq!(result["musicWatchProperties"], 1);
        assert_eq!(result["canonicalStringPaths"], 1);
        assert_eq!(result["bindingValidated"], false);
        assert_eq!(result["ambiguous"], false);
        assert!(!result.to_string().contains("synthetic-private"));
        assert_eq!(
            summarize("<script>x</script>"),
            Err("bootstrap_structure_html_parse_rejected")
        );
    }
}

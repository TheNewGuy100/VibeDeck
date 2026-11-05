//! One separately approved bootstrap + script checkpoint. Never executes JS/player.
use tauri::Manager;
const LIMIT: usize = 4 * 1024 * 1024;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScriptCheckpointBudget {
    auxiliary_get: u8,
    script_get: u8,
}

fn reserve_script_checkpoint(
    path: &std::path::Path,
    budget: &ScriptCheckpointBudget,
) -> Result<(), &'static str> {
    use std::io::Write;
    if budget.auxiliary_get != 1 || budget.script_get != 1 {
        return Err("script_diagnostic_budget_rejected");
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "script_diagnostic_checkpoint_unavailable")?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .map_err(|_| "script_diagnostic_checkpoint_unavailable")
}

fn script_url(path: &str) -> Result<url::Url, &'static str> {
    if path.len() > 512
        || !path.starts_with("/s/player/")
        || !path.ends_with("/base.js")
        || path.split('/').count() < 6
        || path.split('/').skip(1).any(|s| {
            s.is_empty()
                || s == "."
                || s == ".."
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        })
    {
        return Err("script_diagnostic_path_rejected");
    }
    // Resolve root-relative declaration against the exact bootstrap origin.
    url::Url::parse(&format!("https://music.youtube.com{path}"))
        .map_err(|_| "script_diagnostic_path_rejected")
}

#[tauri::command]
pub(crate) async fn debug_youtube_bootstrap_script_once(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: ScriptCheckpointBudget,
) -> Result<serde_json::Value, String> {
    if window.label() != "main" {
        return Err("script_diagnostic_wrong_window".into());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "script_diagnostic_directory_failed")?;
    reserve_script_checkpoint(
        &dir.join("bootstrap-script-checkpoint-20261022.count"),
        &budget,
    )?;
    let mut report = serde_json::json!({"result":"script_diagnostic_pending","bootstrapGetAttempted":false,"scriptGetAttempted":false,"executionPerformed":false,"playerBindingValidated":false});
    if let Err(code) = fetch_checkpoint(&dir, &mut report).await {
        report["result"] = serde_json::json!(code);
    }
    Ok(report)
}

async fn fetch_checkpoint(
    dir: &std::path::Path,
    report: &mut serde_json::Value,
) -> Result<(), String> {
    use vibedeck_bootstrap_inspect::script_diagnostic::{
        inspect_script_timestamp, select_music_script,
    };
    let session = crate::youtube_session::YouTubeSession::load(&dir.join("youtube-session.bin"))
        .map_err(|_| "script_diagnostic_session_unavailable")?;
    let now = crate::unix_now().map_err(|_| "script_diagnostic_clock_failed")?;
    if !session.has_auth_cookie(now) {
        return Err("script_diagnostic_session_ineligible".into());
    }
    let cookie = session
        .cookie_header_for("https://music.youtube.com/", now)
        .map_err(|_| "script_diagnostic_session_ineligible")?;
    let client = crate::youtube_http::client()?;
    report["bootstrapGetAttempted"] = serde_json::json!(true);
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
    if response.status() != reqwest::StatusCode::OK {
        return Err("script_diagnostic_bootstrap_status_rejected".into());
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !mime
        .split(';')
        .next()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/html"))
    {
        return Err("script_diagnostic_bootstrap_mime_rejected".into());
    }
    let bytes = crate::youtube_http::bounded_body(response, LIMIT).await?;
    let html =
        std::str::from_utf8(&bytes).map_err(|_| "script_diagnostic_bootstrap_encoding_rejected")?;
    let declaration = select_music_script(html)?;
    let url = script_url(declaration.path())?;
    report["bootstrapBytes"] = serde_json::json!(bytes.len());
    report["declarationSelected"] = serde_json::json!(true);
    // No cookie jar, default auth headers, redirects or retries. Never forward Cookie.
    report["scriptGetAttempted"] = serde_json::json!(true);
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| crate::youtube_http::request_error(&e))?;
    if response.status() != reqwest::StatusCode::OK || response.url() != &url {
        return Err("script_diagnostic_script_status_rejected".into());
    }
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
        "text/javascript" | "application/javascript"
    ) {
        return Err("script_diagnostic_script_mime_rejected".into());
    }
    let bytes = crate::youtube_http::bounded_body(response, LIMIT).await?;
    let source =
        std::str::from_utf8(&bytes).map_err(|_| "script_diagnostic_script_encoding_rejected")?;
    report["scriptBytes"] = serde_json::json!(bytes.len());
    let inventory = inspect_script_timestamp(source)?;
    report["timestampProperties"] = serde_json::json!(inventory.properties);
    report["timestampIntegerLiterals"] = serde_json::json!(inventory.bounded_integer_literals);
    report["coverage"] = serde_json::json!("declaration_provenance_and_script_ast_only");
    report["result"] = serde_json::json!("script_diagnostic_inspected");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_path_cannot_change_origin_or_add_query() {
        assert_eq!(
            script_url("/s/player/synthetic/x/base.js")
                .unwrap()
                .host_str(),
            Some("music.youtube.com")
        );
        for path in [
            "//other/base.js",
            "/s/player/../x/base.js",
            "/s/player/%2e/x/base.js",
            "/s/player/synthetic/x/base.js?q=private",
        ] {
            assert!(script_url(path).is_err());
        }
    }
    #[test]
    fn persistent_checkpoint_is_exact_and_cannot_reopen_after_restart() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vibedeck-script-budget-{}-{nonce}.count",
            std::process::id()
        ));
        assert!(serde_json::from_str::<ScriptCheckpointBudget>(
            r#"{"auxiliaryGet":1,"scriptGet":1,"playerPost":1}"#
        )
        .is_err());
        assert!(reserve_script_checkpoint(
            &path,
            &ScriptCheckpointBudget {
                auxiliary_get: 2,
                script_get: 1
            }
        )
        .is_err());
        assert!(!path.exists());
        assert!(reserve_script_checkpoint(
            &path,
            &ScriptCheckpointBudget {
                auxiliary_get: 1,
                script_get: 1
            }
        )
        .is_ok());
        assert!(reserve_script_checkpoint(
            &path,
            &ScriptCheckpointBudget {
                auxiliary_get: 1,
                script_get: 1
            }
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"\n");
        std::fs::remove_file(path).unwrap();
    }
}

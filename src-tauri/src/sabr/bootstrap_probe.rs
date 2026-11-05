//! One explicitly approved bootstrap GET. Lexical hints only, never a script resolver.
use std::sync::atomic::{AtomicBool, Ordering};

const BODY_LIMIT: usize = 4 * 1024 * 1024;
const HINT_LIMIT: usize = 32;
static ATTEMPTED: AtomicBool = AtomicBool::new(false);

#[derive(serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BootstrapBudget {
    auxiliary_get: u8,
}

fn reserve(attempted: &AtomicBool, budget: &BootstrapBudget) -> Result<(), &'static str> {
    if budget.auxiliary_get != 1 {
        return Err("bootstrap_probe_budget_rejected");
    }
    attempted
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "bootstrap_probe_already_attempted")?;
    Ok(())
}

/// Values never leave this function. Matches can occur in comments/strings or inert
/// markup: counts are discovery hints, NOT HTML/JS parse or revision binding.
fn summarize(source: &str) -> Result<serde_json::Value, &'static str> {
    if source.len() > BODY_LIMIT {
        return Err("bootstrap_probe_body_limit");
    }
    let mut keys = Vec::new();
    for key in ["PLAYER_JS_URL", "jsUrl", "js", "STS"] {
        let pattern = format!(
            r#""{}"\s*:\s*(?P<value>"(?:\\.|[^"\\])*"|[^\s,}}]{{1,64}})"#,
            key
        );
        let regex = regex::Regex::new(&pattern).map_err(|_| "bootstrap_probe_pattern_failed")?;
        let mut types = [0usize; 8];
        let mut paths = std::collections::HashSet::new();
        let mut count = 0;
        let mut capped = false;
        for capture in regex.captures_iter(source) {
            if count == HINT_LIMIT {
                capped = true;
                break;
            }
            count += 1;
            let value = &capture["value"];
            let kind = match value.as_bytes()[0] {
                b'"' => 0,
                b'{' => 1,
                b'[' => 2,
                b'0'..=b'9' | b'-' => 3,
                b't' | b'f' => 4,
                b'n' => 5,
                _ => 6,
            };
            types[kind] += 1;
            if kind == 0 {
                if let Ok(decoded) = serde_json::from_str::<String>(value) {
                    if decoded.len() <= 512
                        && decoded.starts_with("/s/player/")
                        && decoded.ends_with("/base.js")
                        && decoded.split('/').skip(1).all(|s| {
                            !s.is_empty()
                                && s != "."
                                && s != ".."
                                && s.bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
                        })
                    {
                        paths.insert(decoded);
                    }
                } else {
                    types[7] += 1;
                }
            }
        }
        keys.push(
            serde_json::json!({"key":key,"matches":count,"capped":capped,
            "string":types[0],"objectStart":types[1],"arrayStart":types[2],
            "numberStart":types[3],"booleanStart":types[4],"nullStart":types[5],
            "other":types[6],"invalidJsonString":types[7],"distinctPathHints":paths.len()}),
        );
    }
    // A path marker may occur in inert markup, comments or an escaped JS string.
    let raw = source.matches("/s/player/").take(HINT_LIMIT + 1).count();
    let escaped = source
        .matches(r"\/s\/player\/")
        .take(HINT_LIMIT + 1)
        .count();
    Ok(
        serde_json::json!({"result":"bootstrap_probe_inspected","coverage":"lexical_only",
        "bodyBytes":source.len(),"keys":keys,"rawPathMarkers":raw.min(HINT_LIMIT),
        "escapedPathMarkers":escaped.min(HINT_LIMIT),"pathMarkersCapped":raw > HINT_LIMIT || escaped > HINT_LIMIT}),
    )
}

#[tauri::command]
pub(crate) async fn debug_youtube_bootstrap_once(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    budget: BootstrapBudget,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    if window.label() != "main" {
        return Err("bootstrap_probe_wrong_window".into());
    }
    reserve(&ATTEMPTED, &budget)?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| "bootstrap_probe_directory_failed")?
        .join("youtube-session.bin");
    let session = crate::youtube_session::YouTubeSession::load(&path)
        .map_err(|_| "bootstrap_probe_session_unavailable")?;
    let now = crate::unix_now().map_err(|_| "bootstrap_probe_clock_failed")?;
    if !session.has_auth_cookie(now) {
        return Err("bootstrap_probe_recapture_required".into());
    }
    let cookie = session
        .cookie_header_for("https://music.youtube.com/", now)
        .map_err(|_| "bootstrap_probe_session_ineligible")?;
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
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/html"))
        });
    if !html {
        return Err("bootstrap_probe_mime_rejected".into());
    }
    let bytes = crate::youtube_http::bounded_body(response, BODY_LIMIT).await?;
    let source = std::str::from_utf8(&bytes).map_err(|_| "bootstrap_probe_utf8_rejected")?;
    summarize(source).map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_and_failure_reservation_are_single_use() {
        let attempted = AtomicBool::new(false);
        assert!(
            serde_json::from_str::<BootstrapBudget>(r#"{"auxiliaryGet":1,"playerPost":1}"#)
                .is_err()
        );
        assert_eq!(
            reserve(&attempted, &BootstrapBudget { auxiliary_get: 0 }),
            Err("bootstrap_probe_budget_rejected")
        );
        assert!(reserve(&attempted, &BootstrapBudget { auxiliary_get: 1 }).is_ok());
        assert_eq!(
            reserve(&attempted, &BootstrapBudget { auxiliary_get: 1 }),
            Err("bootstrap_probe_already_attempted")
        );
    }
    #[test]
    fn projection_never_exports_values_and_distinguishes_lexical_types() {
        let source = r#"{"PLAYER_JS_URL":"\/s\/player\/synthetic-private\/x\/base.js","js":null,"js":{},"STS":123}"#;
        let summary = summarize(source).unwrap();
        assert_eq!(summary["keys"][0]["distinctPathHints"], 1);
        assert_eq!(summary["keys"][2]["matches"], 2);
        assert_eq!(summary["keys"][2]["nullStart"], 1);
        assert_eq!(summary["keys"][2]["objectStart"], 1);
        assert!(!summary.to_string().contains("synthetic-private"));
        assert_eq!(summary["coverage"], "lexical_only");
    }
    #[test]
    fn hints_in_comments_are_not_promoted_to_declarations_and_counts_are_bounded() {
        let summary =
            summarize(&r#"<!-- "jsUrl":"/s/player/synthetic/x/base.js" -->"#.repeat(40)).unwrap();
        assert_eq!(summary["coverage"], "lexical_only");
        assert_eq!(summary["keys"][1]["matches"], HINT_LIMIT);
        assert_eq!(summary["keys"][1]["capped"], true);
        assert_eq!(summary["keys"][1]["distinctPathHints"], 1);
        assert!(summarize(&"x".repeat(BODY_LIMIT + 1)).is_err());
    }
}

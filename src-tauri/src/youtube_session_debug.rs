//! Explicit, Windows debug-only session operations. No provider HTTP or playback.
use super::youtube_session::{unix_now, ScopedCookie, YouTubeSession};
use serde::{Deserialize, Serialize};
use tauri::Manager;

static OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    OpenLogin,
    CaptureSave,
    VerifySnapshot,
    RestoreVerify,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    stage: &'static str,
    cookie_count: usize,
    host_only_count: usize,
    persistent_count: usize,
    auth_eligible: bool,
    compared_count: usize,
    missing_count: usize,
    changed_count: usize,
    unexpected_count: usize,
    matched: bool,
    comparison_performed: bool,
}

fn compare(expected: &[ScopedCookie], actual: &[ScopedCookie]) -> Report {
    let mut report = Report {
        compared_count: expected.len(),
        comparison_performed: true,
        ..Report::default()
    };
    for cookie in expected {
        let found = actual.iter().find(|other| {
            other.name == cookie.name && other.domain == cookie.domain && other.path == cookie.path
        });
        match found {
            None => report.missing_count += 1,
            Some(other) if other != cookie => report.changed_count += 1,
            _ => {}
        }
    }
    report.unexpected_count = actual
        .iter()
        .filter(|other| {
            !expected.iter().any(|cookie| {
                cookie.name == other.name
                    && cookie.domain == other.domain
                    && cookie.path == other.path
            })
        })
        .count();
    report.matched =
        report.missing_count == 0 && report.changed_count == 0 && report.unexpected_count == 0;
    report
}

fn summarize(session: &YouTubeSession, now: i64, mut report: Report) -> Report {
    let cookies = session.restorable_cookies(now);
    report.cookie_count = cookies.len();
    report.host_only_count = cookies.iter().filter(|cookie| cookie.host_only).count();
    report.persistent_count = cookies
        .iter()
        .filter(|cookie| cookie.expires_unix.is_some())
        .count();
    report.auth_eligible = session.has_auth_cookie(now);
    report
}

struct BlankWindow(tauri::WebviewWindow);
impl Drop for BlankWindow {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}

#[tauri::command]
pub(crate) async fn debug_youtube_session(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    action: Action,
) -> Result<Report, String> {
    // Never expose this entry point to the provider/login webview.
    if window.label() != "main" {
        return Err("session_debug_wrong_window".into());
    }
    let _guard = OPERATION.try_lock().map_err(|_| "session_debug_busy")?;
    if matches!(action, Action::OpenLogin) {
        if app.get_webview_window("youtube-auth").is_some() {
            return Err("session_debug_login_already_open".into());
        }
        // Direct external entry: never mount App.vue inside the login window.
        let url = "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F"
            .parse().map_err(|_| "session_debug_login_failed")?;
        tauri::WebviewWindowBuilder::new(&app, "youtube-auth", tauri::WebviewUrl::External(url))
            .title("VibeDeck — login de teste")
            .inner_size(520.0, 760.0)
            .build()
            .map_err(|_| "session_debug_login_failed")?;
        return Ok(Report {
            stage: "login_opened",
            ..Report::default()
        });
    }
    let path = app
        .path()
        .app_data_dir()
        .map_err(|_| "session_debug_directory_failed")?
        .join("youtube-session.bin");
    if matches!(action, Action::CaptureSave) {
        let source = app
            .get_webview_window("youtube-auth")
            .ok_or("session_debug_login_not_open")?;
        let session = super::youtube_cookie_bridge::capture(&source)
            .await
            .map_err(|_| "session_debug_capture_failed")?;
        if !session.has_auth_cookie(unix_now()?) {
            return Err("session_debug_auth_not_ready".into());
        }
        session
            .save(&path)
            .map_err(|_| "session_debug_save_failed")?;
        let loaded = YouTubeSession::load(&path).map_err(|_| "session_debug_reload_failed")?;
        let now = unix_now()?;
        let mut report = compare(
            &session.restorable_cookies(now),
            &loaded.restorable_cookies(now),
        );
        report.stage = "capture_saved_reloaded";
        // Keep login open for user control, do not call the service/UI connect flow.
        return Ok(summarize(&loaded, now, report));
    }
    let session = YouTubeSession::load(&path).map_err(|_| "session_debug_snapshot_unavailable")?;
    if matches!(action, Action::VerifySnapshot) {
        return Ok(summarize(
            &session,
            unix_now()?,
            Report {
                stage: "snapshot_loaded",
                ..Report::default()
            },
        ));
    }
    if !session.has_auth_cookie(unix_now()?) {
        return Err("session_debug_auth_not_ready".into());
    }
    // Private profile + blank page: no provider navigation, scripts, or media.
    let blank = BlankWindow(
        tauri::WebviewWindowBuilder::new(
            &app,
            "youtube-session-debug-blank",
            tauri::WebviewUrl::External(
                "about:blank"
                    .parse()
                    .map_err(|_| "session_debug_blank_failed")?,
            ),
        )
        .title("VibeDeck — verificação de sessão")
        .visible(false)
        .incognito(true)
        .on_navigation(|url| url.as_str() == "about:blank")
        .build()
        .map_err(|_| "session_debug_blank_failed")?,
    );
    let before = super::youtube_cookie_bridge::capture(&blank.0)
        .await
        .map_err(|_| "session_debug_baseline_failed")?;
    if !before.restorable_cookies(unix_now()?).is_empty() {
        return Err("session_debug_baseline_not_empty".into());
    }
    super::youtube_cookie_bridge::restore(&blank.0, &session)
        .await
        .map_err(|_| "session_debug_restore_failed")?;
    let after = super::youtube_cookie_bridge::capture(&blank.0)
        .await
        .map_err(|_| "session_debug_readback_failed")?;
    let now = unix_now()?;
    let mut report = compare(
        &session.restorable_cookies(now),
        &after.restorable_cookies(now),
    );
    report.stage = "restore_readback";
    Ok(summarize(&after, now, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::youtube_session::CookieSameSite;

    fn cookie() -> ScopedCookie {
        ScopedCookie {
            name: "synthetic".into(),
            value: "private-synthetic".into(),
            domain: "youtube.com".into(),
            host_only: false,
            path: "/".into(),
            secure: true,
            http_only: true,
            same_site: CookieSameSite::Lax,
            expires_unix: None,
        }
    }

    #[test]
    fn comparison_checks_values_and_all_metadata_without_serializing_them() {
        let expected = cookie();
        assert!(compare(&[expected.clone()], &[expected.clone()]).matched);
        let variants: Vec<Box<dyn Fn(&mut ScopedCookie)>> = vec![
            Box::new(|c| c.value = "changed".into()),
            Box::new(|c| c.host_only = true),
            Box::new(|c| c.secure = false),
            Box::new(|c| c.http_only = false),
            Box::new(|c| c.same_site = CookieSameSite::Strict),
            Box::new(|c| c.expires_unix = Some(123)),
        ];
        for change in variants {
            let mut actual = expected.clone();
            change(&mut actual);
            let report = compare(&[expected.clone()], &[actual]);
            assert_eq!(report.changed_count, 1);
            assert!(!report.matched);
        }
        let json = serde_json::to_string(&compare(&[expected.clone()], &[expected])).unwrap();
        for secret in [
            "private-synthetic",
            "youtube.com",
            "domain",
            "path",
            "value",
        ] {
            assert!(!json.contains(secret));
        }
    }

    #[test]
    fn missing_unexpected_and_order_are_distinguished() {
        let a = cookie();
        let mut b = a.clone();
        b.path = "/other".into();
        assert!(compare(&[a.clone(), b.clone()], &[b.clone(), a.clone()]).matched);
        let report = compare(&[a], &[b]);
        assert_eq!(report.missing_count, 1);
        assert_eq!(report.unexpected_count, 1);
        assert!(!report.matched);
    }

    #[test]
    fn action_allowlist_rejects_arbitrary_commands() {
        assert!(serde_json::from_str::<Action>("\"restore_verify\"").is_ok());
        assert!(serde_json::from_str::<Action>("\"play\"").is_err());
    }
}

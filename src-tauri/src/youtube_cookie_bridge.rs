//! Windows-only boundary: retain WebView2's raw domain (leading dot matters).
//! Wry's cookie::Cookie conversion strips it, so do not roundtrip through it.
use super::youtube_session::{
    is_youtube_cookie_domain, unix_now, CookieSameSite, ScopedCookie, YouTubeSession,
};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{CoTaskMemPWSTR, GetCookiesCompletedHandler};
use windows_core::{Interface, HSTRING, PCWSTR, PWSTR};

fn domain_scope(raw_domain: &str) -> Option<(String, bool)> {
    if !is_youtube_cookie_domain(raw_domain) {
        return None;
    }
    Some((
        raw_domain
            .strip_prefix('.')
            .unwrap_or(raw_domain)
            .to_ascii_lowercase(),
        !raw_domain.starts_with('.'),
    ))
}

unsafe fn read_string(
    read: impl FnOnce(*mut PWSTR) -> windows_core::Result<()>,
) -> windows_core::Result<String> {
    let mut raw = PWSTR::null();
    let result = read(&mut raw);
    let owned = CoTaskMemPWSTR::from(raw);
    result?;
    Ok(owned.to_string())
}

unsafe fn read_cookie(cookie: &ICoreWebView2Cookie) -> windows_core::Result<Option<ScopedCookie>> {
    let raw_domain = read_string(|out| cookie.Domain(out))?;
    let Some((domain, host_only)) = domain_scope(&raw_domain) else {
        return Ok(None);
    };
    let mut secure = false.into();
    let mut http_only = false.into();
    let mut is_session = false.into();
    cookie.IsSecure(&mut secure)?;
    cookie.IsHttpOnly(&mut http_only)?;
    cookie.IsSession(&mut is_session)?;
    let mut same_site = COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX;
    cookie.SameSite(&mut same_site)?;
    let same_site = match same_site {
        COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT => CookieSameSite::Strict,
        COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE => CookieSameSite::None,
        _ => CookieSameSite::Lax,
    };
    let mut expires = 0.0;
    cookie.Expires(&mut expires)?;
    if !is_session.as_bool() && (!expires.is_finite() || expires < 0.0 || expires > i64::MAX as f64)
    {
        return Err(windows_core::Error::from_hresult(windows_core::HRESULT(
            0x80004005u32 as i32,
        )));
    }
    Ok(Some(ScopedCookie {
        name: read_string(|out| cookie.Name(out))?,
        value: read_string(|out| cookie.Value(out))?,
        domain,
        host_only,
        path: read_string(|out| cookie.Path(out))?,
        secure: secure.as_bool(),
        http_only: http_only.as_bool(),
        same_site,
        expires_unix: (!is_session.as_bool()).then_some(expires.floor() as i64),
    }))
}

pub(super) async fn capture(window: &tauri::WebviewWindow) -> Result<YouTubeSession, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .with_webview(move |webview| {
            let callback_tx = tx.clone();
            let result = (|| unsafe {
                let core = webview
                    .controller()
                    .CoreWebView2()?
                    .cast::<ICoreWebView2_2>()?;
                core.CookieManager()?.GetCookies(
                    PCWSTR::null(),
                    &GetCookiesCompletedHandler::create(Box::new(move |status, cookies| {
                        let result = (|| {
                            status?;
                            let cookies = cookies.ok_or_else(|| {
                                windows_core::Error::from_hresult(windows_core::HRESULT(
                                    0x80004005u32 as i32,
                                ))
                            })?;
                            let mut count = 0;
                            cookies.Count(&mut count)?;
                            let mut out = Vec::new();
                            for index in 0..count {
                                if let Some(cookie) = read_cookie(&cookies.GetValueAtIndex(index)?)?
                                {
                                    out.push(cookie);
                                }
                            }
                            Ok::<_, windows_core::Error>(out)
                        })()
                        .map_err(|_| "youtube_cookie_capture_failed".to_string());
                        let _ = callback_tx.send(result);
                        Ok(())
                    })),
                )
            })();
            if result.is_err() {
                let _ = tx.send(Err("youtube_cookie_capture_failed".into()));
            }
        })
        .map_err(|_| "youtube_cookie_dispatch_failed")?;
    let cookies = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|_| "youtube_cookie_capture_failed")?
    .map_err(|_| "youtube_cookie_capture_timeout")??;
    let result = YouTubeSession::from_cookies(cookies.clone(), unix_now()?);
    match &result {
        Err(code) if code == "youtube_session_ambiguous_cookie" => log_conflict_once(window, &cookies),
        Ok(session) if !session.ignored_ambiguous().is_empty() => log_once(
            window,
            &format!(
                "YouTube login ignored cookies whose copies disagree (not needed for sign-in): {}",
                session.ignored_ambiguous().join(", ")
            ),
        ),
        _ => {}
    }
    result
}

fn log_conflict_once(window: &tauri::WebviewWindow, cookies: &[ScopedCookie]) {
    if let Some(detail) = YouTubeSession::describe_conflict(cookies) {
        log_once(
            window,
            &format!("YouTube login cookie conflict (failed to merge): {detail}"),
        );
    }
}

/// The wait polls every second; report a given message only when it changes.
fn log_once(window: &tauri::WebviewWindow, message: &str) {
    use std::sync::{Mutex, OnceLock};
    use tauri::Manager;
    static LAST: OnceLock<Mutex<String>> = OnceLock::new();
    if let Ok(mut last) = LAST.get_or_init(|| Mutex::new(String::new())).lock() {
        if last.as_str() != message {
            super::write_native_log(window.app_handle(), message);
            *last = message.to_string();
        }
    }
}

pub(super) async fn restore(
    window: &tauri::WebviewWindow,
    session: &YouTubeSession,
) -> Result<(), String> {
    let cookies = session.restorable_cookies(unix_now()?);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let (tx, rx) = std::sync::mpsc::channel();
    window
        .with_webview(move |webview| {
            let result = (|| unsafe {
                let core = webview
                    .controller()
                    .CoreWebView2()?
                    .cast::<ICoreWebView2_2>()?;
                let manager = core.CookieManager()?;
                for cookie in cookies {
                    if std::time::Instant::now() >= deadline {
                        return Err(windows_core::Error::from_hresult(windows_core::HRESULT(
                            0x80004005u32 as i32,
                        )));
                    }
                    // Recheck on the UI thread: dispatch may have taken time.
                    if cookie
                        .expires_unix
                        .is_some_and(|expiry| unix_now().map_or(true, |now| expiry <= now))
                    {
                        continue;
                    }
                    let domain = if cookie.host_only {
                        cookie.domain
                    } else {
                        format!(".{}", cookie.domain)
                    };
                    let native = manager.CreateCookie(
                        &HSTRING::from(cookie.name),
                        &HSTRING::from(cookie.value),
                        &HSTRING::from(domain),
                        &HSTRING::from(cookie.path),
                    )?;
                    native.SetIsSecure(cookie.secure)?;
                    native.SetIsHttpOnly(cookie.http_only)?;
                    native.SetSameSite(match cookie.same_site {
                        CookieSameSite::None => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE,
                        CookieSameSite::Lax => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX,
                        CookieSameSite::Strict => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT,
                    })?;
                    if let Some(expiry) = cookie.expires_unix {
                        native.SetExpires(expiry as f64)?;
                    }
                    manager.AddOrUpdateCookie(&native)?;
                }
                Ok::<_, windows_core::Error>(())
            })()
            .map_err(|_| "youtube_cookie_restore_failed".to_string());
            let _ = tx.send(result);
        })
        .map_err(|_| "youtube_cookie_dispatch_failed")?;
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|_| "youtube_cookie_restore_failed")?
    .map_err(|_| "youtube_cookie_restore_timeout")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_webview_domain_preserves_host_only_semantics() {
        assert_eq!(
            domain_scope(".YouTube.COM"),
            Some(("youtube.com".into(), false))
        );
        assert_eq!(
            domain_scope("music.youtube.com"),
            Some(("music.youtube.com".into(), true))
        );
        assert_eq!(
            domain_scope(".music.youtube.com"),
            Some(("music.youtube.com".into(), false))
        );
        for domain in [
            "notyoutube.com",
            "..youtube.com",
            "youtube.com.evil.test",
            "",
        ] {
            assert!(domain_scope(domain).is_none());
        }
    }
}

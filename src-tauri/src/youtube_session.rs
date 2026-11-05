//! Native credential boundary. No WebView, requests, or playback lives here.
//! No Debug or frontend serialization. Disk serialization is DPAPI-only.
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::path::Path;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScopedCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: CookieSameSite,
    pub expires_unix: Option<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum CookieSameSite {
    None,
    Lax,
    Strict,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u32,
    cookies: Vec<ScopedCookie>,
}

pub(super) struct YouTubeSession {
    cookies: Vec<ScopedCookie>,
}

impl YouTubeSession {
    #[cfg(test)]
    pub(super) fn from_cookie_header(cookie_header: String) -> Self {
        Self::from_cookies(
            cookie_header
                .split(';')
                .filter_map(|item| {
                    let (name, value) = item.trim().split_once('=')?;
                    Some(ScopedCookie {
                        name: name.into(),
                        value: value.into(),
                        domain: "youtube.com".into(),
                        host_only: false,
                        path: "/".into(),
                        secure: true,
                        http_only: true,
                        same_site: CookieSameSite::None,
                        expires_unix: None,
                    })
                })
                .collect(),
            0,
        )
        .unwrap()
    }

    pub(super) fn from_cookies(cookies: Vec<ScopedCookie>, now: i64) -> Result<Self, String> {
        let mut kept: Vec<ScopedCookie> = Vec::new();
        for mut cookie in cookies {
            cookie.domain = cookie.domain.to_ascii_lowercase();
            if !is_youtube_cookie_domain(&cookie.domain) || cookie.domain.starts_with('.') {
                continue;
            }
            if !valid_cookie(&cookie) {
                return Err("youtube_session_invalid_cookie".into());
            }
            if cookie.expires_unix.is_some_and(|expiry| expiry <= now) {
                continue;
            }
            if let Some(previous) = kept.iter().find(|previous| {
                previous.name == cookie.name
                    && previous.domain == cookie.domain
                    && previous.path == cookie.path
            }) {
                // Conflicting duplicate identities have no trustworthy capture order.
                if previous != &cookie {
                    return Err("youtube_session_ambiguous_cookie".into());
                }
                continue;
            }
            kept.push(cookie);
        }
        Ok(Self { cookies: kept })
    }

    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let protected = std::fs::read(path).map_err(|_| "youtube_session_read_failed")?;
        let bytes =
            super::protect_data(protected, true).map_err(|_| "youtube_session_unprotect_failed")?;
        Self::decode_snapshot(&bytes)
    }

    fn decode_snapshot(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "youtube_session_invalid_encoding")?;
        if !text.trim_start().starts_with('{') {
            // A legacy header cannot recover domain/path/expiry. Never rewrite it.
            return Err("youtube_session_recapture_required".into());
        }
        let snapshot: Snapshot =
            serde_json::from_str(text).map_err(|_| "youtube_session_invalid_snapshot")?;
        if snapshot.version != 2 {
            return Err("youtube_session_unsupported_version".into());
        }
        Self::from_cookies(snapshot.cookies, unix_now()?)
    }

    pub(super) fn save(&self, path: &Path) -> Result<(), String> {
        use std::io::Write;
        let bytes = serde_json::to_vec(&Snapshot {
            version: 2,
            cookies: self.cookies.clone(),
        })
        .map_err(|_| "youtube_session_encode_failed")?;
        let protected =
            super::protect_data(bytes, false).map_err(|_| "youtube_session_protect_failed")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "youtube_session_directory_failed")?;
        }
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "youtube_session_clock_failed")?
            .as_nanos();
        let temporary = path.with_extension(format!("{}-{suffix}.tmp", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "youtube_session_write_failed")?;
        let written = file.write_all(&protected).and_then(|_| file.sync_all());
        drop(file);
        let result = written.and_then(|_| std::fs::rename(&temporary, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|_| "youtube_session_write_failed".into())
    }

    pub(super) fn cookie_header_for(&self, url: &str, now: i64) -> Result<String, String> {
        let cookies = self.cookies_for(url, now)?;
        Ok(cookies
            .iter()
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; "))
    }

    fn cookies_for(&self, url: &str, now: i64) -> Result<Vec<&ScopedCookie>, String> {
        let url = url::Url::parse(url).map_err(|_| "youtube_session_invalid_target")?;
        let host = url.host_str().ok_or("youtube_session_invalid_target")?;
        if url.scheme() != "https"
            || !is_youtube_cookie_domain(host)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err("youtube_session_invalid_target".into());
        }
        let mut cookies: Vec<_> = self
            .cookies
            .iter()
            .filter(|cookie| {
                let domain_matches = host == cookie.domain
                    || (!cookie.host_only
                        && host
                            .strip_suffix(&cookie.domain)
                            .is_some_and(|prefix| prefix.ends_with('.')));
                let path = url.path();
                let path_matches = path == cookie.path
                    || path.strip_prefix(&cookie.path).is_some_and(|suffix| {
                        cookie.path.ends_with('/') || suffix.starts_with('/')
                    });
                domain_matches
                    && path_matches
                    && cookie.expires_unix.is_none_or(|expiry| expiry > now)
            })
            .collect();
        // RFC path priority; stable capture order for equally specific cookies.
        cookies.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));
        Ok(cookies)
    }

    pub(super) fn restorable_cookies(&self, now: i64) -> Vec<ScopedCookie> {
        self.cookies
            .iter()
            .filter(|cookie| cookie.expires_unix.is_none_or(|expiry| expiry > now))
            .cloned()
            .collect()
    }

    pub(super) fn has_auth_cookie(&self, now: i64) -> bool {
        let header = self
            .cookie_header_for("https://music.youtube.com/youtubei/v1/player", now)
            .unwrap_or_default();
        cookie_value(&header, "SAPISID")
            .or_else(|| cookie_value(&header, "__Secure-3PAPISID"))
            .is_some()
            && u64::try_from(now).ok().is_some_and(|timestamp| {
                self.authorization(
                    "https://music.youtube.com/youtubei/v1/player",
                    timestamp,
                    None,
                )
                .is_ok()
            })
    }

    /// Explicit optional session identity, never inferred from account index.
    /// Public reverse-engineered behavior; server acceptance is not guaranteed.
    pub(super) fn authorization(
        &self,
        url: &str,
        timestamp: u64,
        user_session_id: Option<&str>,
    ) -> Result<String, String> {
        let origin = "https://music.youtube.com";
        if url::Url::parse(url)
            .ok()
            .is_none_or(|url| url.origin().ascii_serialization() != origin)
        {
            return Err("youtube_session_invalid_target".into());
        }
        let cookies = self.cookies_for(
            url,
            i64::try_from(timestamp).map_err(|_| "youtube_session_invalid_time")?,
        )?;
        let auth_cookie = |name: &str| -> Result<Option<&str>, String> {
            let mut matching = cookies
                .iter()
                .filter(|cookie| cookie.name == name && !cookie.value.is_empty());
            let Some(first) = matching.next() else {
                return Ok(None);
            };
            // WebView2 does not expose creation time. Never guess between
            // equally specific auth cookies from different domains.
            if matching
                .any(|cookie| cookie.path.len() == first.path.len() && cookie.value != first.value)
            {
                return Err("youtube_session_ambiguous_auth_cookie".into());
            }
            Ok(Some(first.value.as_str()))
        };
        let user_session_id = user_session_id.filter(|value| !value.is_empty());
        let third_party = auth_cookie("__Secure-3PAPISID")?;
        let primary = auth_cookie("SAPISID")?.or(third_party);
        let mut schemes = Vec::new();
        for (scheme, sid) in [
            ("SAPISIDHASH", primary),
            ("SAPISID1PHASH", auth_cookie("__Secure-1PAPISID")?),
            ("SAPISID3PHASH", third_party),
        ] {
            if let Some(sid) = sid {
                let input = match user_session_id {
                    Some(user) => format!("{user} {timestamp} {sid} {origin}"),
                    None => format!("{timestamp} {sid} {origin}"),
                };
                let hash = format!("{:x}", Sha1::digest(input.as_bytes()));
                let suffix = if user_session_id.is_some() { "_u" } else { "" };
                schemes.push(format!("{scheme} {timestamp}_{hash}{suffix}"));
            }
        }
        if schemes.is_empty() {
            return Err("youtube_session_missing_auth_cookie".into());
        }
        Ok(schemes.join(" "))
    }
}

pub(super) fn unix_now() -> Result<i64, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .ok_or_else(|| "youtube_session_clock_failed".into())
}

fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|item| {
        let (key, value) = item.trim().split_once('=')?;
        (key == name && !value.is_empty()).then_some(value)
    })
}

fn valid_cookie(cookie: &ScopedCookie) -> bool {
    !cookie.name.is_empty()
        && cookie
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        && cookie.value.bytes().all(|b| {
            b == 0x21
                || (0x23..=0x2b).contains(&b)
                || (0x2d..=0x3a).contains(&b)
                || (0x3c..=0x5b).contains(&b)
                || (0x5d..=0x7e).contains(&b)
        })
        && cookie.path.starts_with('/')
        && !cookie.path.chars().any(|c| c.is_control() || c == ';')
        && cookie.domain.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && (!cookie.name.starts_with("__Secure-") || cookie.secure)
        && (!cookie.name.starts_with("__Host-")
            || (cookie.secure && cookie.host_only && cookie.path == "/"))
}

pub(super) fn is_youtube_cookie_domain(domain: &str) -> bool {
    let domain = domain.strip_prefix('.').unwrap_or(domain);
    if domain.split('.').any(str::is_empty) {
        return false;
    }
    domain.eq_ignore_ascii_case("youtube.com")
        || domain
            .get(..domain.len().saturating_sub(".youtube.com".len()))
            .is_some_and(|prefix| !prefix.is_empty())
            && domain
                .get(domain.len().saturating_sub(".youtube.com".len())..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".youtube.com"))
}

#[cfg(test)]
mod tests {
    use super::*;
    const PLAYER: &str = "https://music.youtube.com/youtubei/v1/player";

    fn session(header: &str) -> YouTubeSession {
        YouTubeSession::from_cookie_header(header.into())
    }

    #[test]
    fn domain_boundary() {
        for domain in [
            "youtube.com",
            ".youtube.com",
            "music.youtube.com",
            ".MUSIC.YouTube.COM",
        ] {
            assert!(is_youtube_cookie_domain(domain));
        }
        for domain in [
            "",
            ".",
            "com",
            "notyoutube.com",
            ".notyoutube.com",
            "youtube.com.example.org",
            "youtube.com.",
            "..youtube.com",
        ] {
            assert!(!is_youtube_cookie_domain(domain));
        }
    }

    #[test]
    fn primary_only_has_no_unbound_user_suffix() {
        let auth = session("SAPISID=synthetic")
            .authorization(PLAYER, 123, None)
            .unwrap();
        assert_eq!(
            auth,
            format!(
                "SAPISIDHASH 123_{:x}",
                Sha1::digest(b"123 synthetic https://music.youtube.com")
            )
        );
        assert!(!auth.ends_with("_u"));
    }

    #[test]
    fn schemes_use_their_own_cookie() {
        let auth = session("SAPISID=primary; __Secure-1PAPISID=first; __Secure-3PAPISID=third")
            .authorization(PLAYER, 123, None)
            .unwrap();
        let expected = [
            ("SAPISIDHASH", "primary"),
            ("SAPISID1PHASH", "first"),
            ("SAPISID3PHASH", "third"),
        ]
        .map(|(scheme, sid)| {
            format!(
                "{scheme} 123_{:x}",
                Sha1::digest(format!("123 {sid} https://music.youtube.com").as_bytes())
            )
        })
        .join(" ");
        assert_eq!(auth, expected);
    }

    #[test]
    fn third_party_fallback_and_empty_cookie() {
        let value = session("SAPISID=; __Secure-3PAPISID=third");
        assert!(value.has_auth_cookie(123));
        let auth = value.authorization(PLAYER, 123, None).unwrap();
        let parts: Vec<_> = auth.split_whitespace().collect();
        assert_eq!(parts[0], "SAPISIDHASH");
        assert_eq!(parts[2], "SAPISID3PHASH");
        assert_eq!(parts[1], parts[3]);
        assert!(session("SAPISID=; PREF=synthetic")
            .authorization(PLAYER, 123, None)
            .is_err());
    }

    #[test]
    fn optional_identity_is_bound_into_digest() {
        let auth = session("SAPISID=synthetic")
            .authorization(PLAYER, 123, Some("synthetic-user"))
            .unwrap();
        assert_eq!(
            auth,
            format!(
                "SAPISIDHASH 123_{:x}_u",
                Sha1::digest(b"synthetic-user 123 synthetic https://music.youtube.com")
            )
        );
        assert_eq!(
            session("SAPISID=synthetic")
                .authorization(PLAYER, 123, Some(""))
                .unwrap(),
            session("SAPISID=synthetic")
                .authorization(PLAYER, 123, None)
                .unwrap()
        );
    }

    #[test]
    fn cookie_value_preserves_equals_and_exact_names() {
        let value = session("NOTSAPISID=wrong; SAPISID=a=b==; PREF=synthetic");
        let header = value.cookie_header_for(PLAYER, 123).unwrap();
        assert_eq!(cookie_value(&header, "SAPISID"), Some("a=b=="));
        assert!(!session("NOTSAPISID=wrong").has_auth_cookie(123));
    }

    #[test]
    fn encrypted_legacy_header_roundtrip_and_safe_errors() {
        let directory = std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap())
            .join("Temp/opencode")
            .join(format!(
                "vibedeck-session-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let path = directory.join("session.bin");
        assert!(
            matches!(YouTubeSession::load(&path), Err(error) if error == "youtube_session_read_failed")
        );
        let original = session("SAPISID=synthetic-only; PREF=synthetic");
        original.save(&path).unwrap();
        // Replacing an existing snapshot must also succeed atomically.
        original.save(&path).unwrap();
        let encrypted = std::fs::read(&path).unwrap();
        assert_ne!(
            encrypted,
            original.cookie_header_for(PLAYER, 123).unwrap().as_bytes()
        );
        assert_eq!(
            YouTubeSession::load(&path)
                .unwrap()
                .cookie_header_for(PLAYER, 123)
                .unwrap(),
            original.cookie_header_for(PLAYER, 123).unwrap()
        );
        let legacy =
            super::super::protect_data(b"SAPISID=synthetic-legacy".to_vec(), false).unwrap();
        std::fs::write(&path, &legacy).unwrap();
        assert!(
            matches!(YouTubeSession::load(&path), Err(error) if error == "youtube_session_recapture_required")
        );
        assert_eq!(std::fs::read(&path).unwrap(), legacy);
        std::fs::write(&path, b"invalid-synthetic-protected-data").unwrap();
        assert!(
            matches!(YouTubeSession::load(&path), Err(error) if error == "youtube_session_unprotect_failed")
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn scoped_selection_preserves_duplicate_names_and_path_priority() {
        let mut root = session("SAPISID=root").cookies.remove(0);
        let mut specific = root.clone();
        specific.path = "/youtubei".into();
        specific.value = "specific".into();
        let mut host = root.clone();
        host.domain = "www.youtube.com".into();
        host.host_only = true;
        host.value = "other-host".into();
        root.expires_unix = Some(200);
        let value = YouTubeSession::from_cookies(vec![root, specific, host], 100).unwrap();
        assert_eq!(
            value.cookie_header_for(PLAYER, 123).unwrap(),
            "SAPISID=specific; SAPISID=root"
        );
        assert_eq!(
            value
                .cookie_header_for("https://music.youtube.com/", 123)
                .unwrap(),
            "SAPISID=root"
        );
        assert_eq!(
            value
                .cookie_header_for("https://music.youtube.com/youtubeix", 123)
                .unwrap(),
            "SAPISID=root"
        );
        assert_eq!(
            value.cookie_header_for(PLAYER, 200).unwrap(),
            "SAPISID=specific"
        );
        assert_eq!(value.restorable_cookies(200).len(), 2);
        assert_eq!(
            value.authorization(PLAYER, 123, None).unwrap(),
            session("SAPISID=specific")
                .authorization(PLAYER, 123, None)
                .unwrap()
        );
    }

    #[test]
    fn host_only_never_reaches_subdomains_and_targets_are_restricted() {
        let mut cookie = session("SAPISID=synthetic").cookies.remove(0);
        cookie.host_only = true;
        let value = YouTubeSession::from_cookies(vec![cookie], 0).unwrap();
        assert!(value.cookie_header_for(PLAYER, 123).unwrap().is_empty());
        assert_eq!(
            value
                .cookie_header_for("https://youtube.com/", 123)
                .unwrap(),
            "SAPISID=synthetic"
        );
        for url in [
            "http://youtube.com/",
            "https://notyoutube.com/",
            "https://youtube.com.evil.test/",
            "https://music.youtube.com:8443/",
            "https://user@youtube.com/",
        ] {
            assert!(value.cookie_header_for(url, 123).is_err());
        }
    }

    #[test]
    fn duplicate_identity_is_deduplicated_or_rejected_not_last_wins() {
        let cookie = session("SAPISID=synthetic").cookies.remove(0);
        assert_eq!(
            YouTubeSession::from_cookies(vec![cookie.clone(), cookie.clone()], 0)
                .unwrap()
                .cookies
                .len(),
            1
        );
        let mut conflicting = cookie.clone();
        conflicting.value = "different".into();
        assert!(
            matches!(YouTubeSession::from_cookies(vec![cookie, conflicting], 0), Err(error) if error == "youtube_session_ambiguous_cookie")
        );
    }

    #[test]
    fn equally_specific_auth_from_distinct_domains_requires_unambiguous_values() {
        let cookie = session("SAPISID=synthetic").cookies.remove(0);
        let mut host = cookie.clone();
        host.domain = "music.youtube.com".into();
        host.host_only = true;
        host.value = "different-synthetic".into();
        let value = YouTubeSession::from_cookies(vec![cookie, host], 0).unwrap();
        assert_eq!(
            value.cookie_header_for(PLAYER, 123).unwrap(),
            "SAPISID=synthetic; SAPISID=different-synthetic"
        );
        assert!(
            matches!(value.authorization(PLAYER, 123, None), Err(error) if error == "youtube_session_ambiguous_auth_cookie")
        );
    }

    #[test]
    fn metadata_roundtrip_and_validation_are_fail_closed() {
        let mut cookie = session("SAPISID=synthetic").cookies.remove(0);
        cookie.domain = "music.youtube.com".into();
        cookie.host_only = true;
        cookie.path = "/youtubei/".into();
        cookie.http_only = true;
        cookie.same_site = CookieSameSite::Strict;
        cookie.expires_unix = Some(i64::MAX);
        let bytes = serde_json::to_vec(&Snapshot {
            version: 2,
            cookies: vec![cookie.clone()],
        })
        .unwrap();
        let decoded = YouTubeSession::decode_snapshot(&bytes).unwrap();
        assert_eq!(
            serde_json::to_vec(&decoded.cookies[0]).unwrap(),
            serde_json::to_vec(&cookie).unwrap()
        );
        cookie.value = "synthetic; injected=value".into();
        assert!(YouTubeSession::from_cookies(vec![cookie], 0).is_err());
        assert!(
            matches!(YouTubeSession::decode_snapshot(br#"{"version":3,"cookies":[]}"#), Err(error) if error == "youtube_session_unsupported_version")
        );
        assert!(
            matches!(YouTubeSession::decode_snapshot(b"{broken"), Err(error) if error == "youtube_session_invalid_snapshot")
        );
    }
}

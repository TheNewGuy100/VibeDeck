//! Acquisition only, never execution. No IPC/startup callers or session access.
//! Explicit approval is still required: a declared budget is not human consent.
const SOURCE_LIMIT: usize = 256 * 1024;

#[cfg(debug_assertions)]
static INSPECTION_ATTEMPTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Only the script GET, not bootstrap/player/SABR or audio. A future IPC must
/// separately enforce main-window origin and the complete operation budget.
#[cfg(debug_assertions)]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InspectionBudget {
    pub(super) script_get: u8,
}

#[cfg(debug_assertions)]
fn reserve_inspection(
    attempted: &std::sync::atomic::AtomicBool,
    budget: InspectionBudget,
) -> Result<(), &'static str> {
    if budget.script_get != 1 {
        return Err("player_script_budget_rejected");
    }
    attempted
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .map_err(|_| "player_script_already_attempted")?;
    Ok(())
}

/// No IPC/UI/startup caller. One acquisition attempt per process including missing
/// identity and network failures; no reset. Caller must supply the approved budget
/// and fresh payload, not reuse historical authorization or restart to bypass it.
#[cfg(debug_assertions)]
pub(crate) async fn inspect_script_once(
    payload: &serde_json::Value,
    budget: InspectionBudget,
) -> Result<AcquiredScript<'_>, &'static str> {
    reserve_inspection(&INSPECTION_ATTEMPTED, budget)?;
    PlayerScriptBinding::from_payload(payload)?.fetch().await
}

/// Borrows the exact player payload that declared the script. No Debug/Serialize,
/// persistent cache, revision guessing, credentials, or cross-bootstrap fallback.
pub(crate) struct PlayerScriptBinding<'a> {
    payload: &'a serde_json::Value,
    url: url::Url,
}

pub(crate) struct AcquiredScript<'a> {
    binding: PlayerScriptBinding<'a>,
    source: String,
}

impl<'a> PlayerScriptBinding<'a> {
    pub(crate) fn from_payload(payload: &'a serde_json::Value) -> Result<Self, &'static str> {
        let path = payload
            .pointer("/assets/js")
            .and_then(|v| v.as_str())
            .ok_or("player_script_identity_missing")?;
        // Deliberately support only a canonical root-relative script path.
        // Reject escapes, dot segments and arbitrary query parameters before URL parsing.
        let segments: Vec<_> = path.split('/').collect();
        if path.len() > 512
            || !path.starts_with("/s/player/")
            || !path.ends_with("/base.js")
            || segments.len() < 6
            || segments.iter().skip(1).any(|s| {
                s.is_empty()
                    || *s == "."
                    || *s == ".."
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
            })
        {
            return Err("player_script_identity_rejected");
        }
        let url = url::Url::parse(&format!("https://www.youtube.com{path}"))
            .map_err(|_| "player_script_identity_rejected")?;
        Ok(Self { payload, url })
    }

    /// One GET, no cookies/auth/redirect/retry, 20 s total and bounded body.
    /// Private: callers must go through the debug-only one-attempt coordinator.
    async fn fetch(self) -> Result<AcquiredScript<'a>, &'static str> {
        let client = crate::youtube_http::client().map_err(|_| "player_script_client_failed")?;
        let response = client
            .get(self.url.clone())
            .send()
            .await
            .map_err(|_| "player_script_transport_failed")?;
        if response.status() != reqwest::StatusCode::OK || response.url() != &self.url {
            return Err("player_script_response_rejected");
        }
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        check_mime(mime)?;
        let bytes = crate::youtube_http::bounded_body(response, SOURCE_LIMIT)
            .await
            .map_err(|_| "player_script_body_failed")?;
        self.accept(&bytes)
    }

    fn accept(self, bytes: &[u8]) -> Result<AcquiredScript<'a>, &'static str> {
        if bytes.is_empty() || bytes.len() > SOURCE_LIMIT {
            return Err("player_script_source_limit");
        }
        let source = std::str::from_utf8(bytes).map_err(|_| "player_script_encoding_rejected")?;
        Ok(AcquiredScript {
            binding: self,
            source: source.to_owned(),
        })
    }
}

impl AcquiredScript<'_> {
    /// Pointer identity, not matching IDs/URLs from unrelated bootstrap operations.
    pub(crate) fn belongs_to(&self, payload: &serde_json::Value) -> bool {
        std::ptr::eq(self.binding.payload, payload)
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }
}

fn check_mime(mime: &str) -> Result<(), &'static str> {
    match mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "application/javascript" | "text/javascript" => Ok(()),
        _ => Err("player_script_mime_rejected"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn payload(path: &str) -> serde_json::Value {
        serde_json::json!({"assets":{"js":path}})
    }
    const PATH: &str = "/s/player/synthetic/player_ias.vflset/en_US/base.js";

    #[test]
    fn inspection_budget_is_exact_and_rejects_extra_operations() {
        for raw in [
            r#"{}"#,
            r#"{"scriptGet":1,"sabrPost":1}"#,
            r#"{"scriptGet":1,"audio":true}"#,
        ] {
            assert!(serde_json::from_str::<InspectionBudget>(raw).is_err());
        }
        let attempted = std::sync::atomic::AtomicBool::new(false);
        for script_get in [0, 2, 255] {
            assert_eq!(
                reserve_inspection(&attempted, InspectionBudget { script_get }),
                Err("player_script_budget_rejected")
            );
        }
        assert_eq!(
            reserve_inspection(&attempted, InspectionBudget { script_get: 1 }),
            Ok(())
        );
        assert_eq!(
            reserve_inspection(&attempted, InspectionBudget { script_get: 1 }),
            Err("player_script_already_attempted")
        );
    }

    #[test]
    fn failed_identity_still_consumes_attempt() {
        let attempted = std::sync::atomic::AtomicBool::new(false);
        reserve_inspection(&attempted, InspectionBudget { script_get: 1 }).unwrap();
        assert!(PlayerScriptBinding::from_payload(&serde_json::json!({})).is_err());
        assert_eq!(
            reserve_inspection(&attempted, InspectionBudget { script_get: 1 }),
            Err("player_script_already_attempted")
        );
    }

    #[test]
    fn concurrent_reservations_have_only_one_winner() {
        let attempted = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let attempted = &attempted;
                    scope.spawn(move || {
                        reserve_inspection(attempted, InspectionBudget { script_get: 1 }).is_ok()
                    })
                })
                .collect();
            assert_eq!(
                handles
                    .into_iter()
                    .map(|h| usize::from(h.join().unwrap()))
                    .sum::<usize>(),
                1
            );
        });
    }

    #[test]
    fn binds_source_to_exact_payload_not_equal_clone() {
        let original = payload(PATH);
        let other = original.clone();
        let script = PlayerScriptBinding::from_payload(&original)
            .unwrap()
            .accept(b"const synthetic=1;")
            .unwrap();
        assert!(script.belongs_to(&original));
        assert!(!script.belongs_to(&other));
        assert_eq!(script.source(), "const synthetic=1;");
        assert_eq!(script.binding.url.host_str(), Some("www.youtube.com"));
    }

    #[test]
    fn rejects_missing_identity_without_guessing() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"assets":{"js":1}}),
        ] {
            assert!(matches!(
                PlayerScriptBinding::from_payload(&value),
                Err("player_script_identity_missing")
            ));
        }
    }

    #[test]
    fn rejects_noncanonical_paths_and_external_destinations() {
        for path in [
            "https://evil.invalid/base.js",
            "//evil.invalid/base.js",
            "/s/player/../x/base.js",
            "/s/player/%2e%2e/x/base.js",
            "/s/player/x/y/base.js?secret=1",
            "/s/player/x/y/base.js#x",
            "/s/player/x\\y/z/base.js",
            "/s/player//y/base.js",
            "/s/player/x/y/other.js",
        ] {
            assert!(PlayerScriptBinding::from_payload(&payload(path)).is_err());
        }
    }

    #[test]
    fn enforces_source_and_mime_limits_without_execution() {
        let value = payload(PATH);
        for bytes in [vec![], vec![b'x'; SOURCE_LIMIT + 1], vec![0xff]] {
            assert!(PlayerScriptBinding::from_payload(&value)
                .unwrap()
                .accept(&bytes)
                .is_err());
        }
        assert!(PlayerScriptBinding::from_payload(&value)
            .unwrap()
            .accept(&vec![b'x'; SOURCE_LIMIT])
            .is_ok());
        assert_eq!(check_mime("Application/JavaScript; charset=utf-8"), Ok(()));
        for mime in [
            "",
            "text/html",
            "application/json",
            "application/javascript-extra",
        ] {
            assert_eq!(check_mime(mime), Err("player_script_mime_rejected"));
        }
    }
}

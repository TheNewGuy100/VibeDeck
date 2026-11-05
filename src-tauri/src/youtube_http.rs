//! Fixed diagnostics only. Never stringify reqwest errors or provider payloads.
pub(super) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // Explicit Cookie headers are selected for the original URL/path only.
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "youtube_http_client_failed".into())
}

pub(super) fn request_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "youtube_http_timeout"
    } else if error.is_connect() {
        "youtube_http_connect_failed"
    } else if error.is_builder() {
        "youtube_http_invalid_request"
    } else if error.is_decode() {
        "youtube_http_decode_failed"
    } else if error.is_body() {
        "youtube_http_body_failed"
    } else {
        "youtube_http_transport_failed"
    }
}

pub(super) fn successful_status(status: reqwest::StatusCode) -> Result<(), String> {
    if status.is_success() {
        Ok(())
    } else {
        Err(match status.as_u16() {
            300..=399 => "youtube_http_redirect_rejected",
            401 | 403 => "youtube_http_access_denied",
            429 => "youtube_http_rate_limited",
            400..=499 => "youtube_http_client_status",
            500..=599 => "youtube_http_server_status",
            _ => "youtube_http_unexpected_status",
        }
        .into())
    }
}

pub(super) async fn bounded_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, String> {
    successful_status(response.status())?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err("youtube_http_body_too_large".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| request_error(&error))?
    {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err("youtube_http_body_too_large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub(super) fn player_json(bytes: &[u8]) -> Result<serde_json::Value, String> {
    let payload: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "youtube_player_invalid_json")?;
    if !payload.is_object() || payload.get("error").is_some() {
        return Err("youtube_player_invalid_payload".into());
    }
    Ok(payload)
}

pub(super) fn playability_status(payload: &serde_json::Value) -> &'static str {
    match payload
        .pointer("/playabilityStatus/status")
        .and_then(|value| value.as_str())
    {
        Some("OK") => "OK",
        Some("UNPLAYABLE") => "UNPLAYABLE",
        Some("LOGIN_REQUIRED") => "LOGIN_REQUIRED",
        Some("ERROR") => "ERROR",
        Some("LIVE_STREAM_OFFLINE") => "LIVE_STREAM_OFFLINE",
        Some("CONTENT_CHECK_REQUIRED") => "CONTENT_CHECK_REQUIRED",
        Some("AGE_CHECK_REQUIRED") => "AGE_CHECK_REQUIRED",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_never_echo_url_headers_or_payload() {
        let error = client()
            .unwrap()
            .get("https://example.invalid/?key=synthetic-private")
            .header("Cookie", "synthetic\nprivate")
            .build()
            .unwrap_err();
        assert_eq!(request_error(&error), "youtube_http_invalid_request");
        assert!(
            matches!(player_json(b"synthetic-private-not-json"), Err(error) if error == "youtube_player_invalid_json")
        );
        assert!(
            matches!(player_json(br#"{"error":{"message":"synthetic-private"}}"#), Err(error) if error == "youtube_player_invalid_payload")
        );
        assert!(player_json(b"[]").is_err());
    }

    #[test]
    fn status_categories_and_playability_are_allowlisted() {
        for (status, expected) in [
            (302, "youtube_http_redirect_rejected"),
            (401, "youtube_http_access_denied"),
            (403, "youtube_http_access_denied"),
            (429, "youtube_http_rate_limited"),
            (404, "youtube_http_client_status"),
            (503, "youtube_http_server_status"),
        ] {
            assert_eq!(
                successful_status(reqwest::StatusCode::from_u16(status).unwrap()).unwrap_err(),
                expected
            );
        }
        assert!(successful_status(reqwest::StatusCode::OK).is_ok());
        assert_eq!(
            playability_status(
                &serde_json::json!({"playabilityStatus":{"status":"synthetic-private"}})
            ),
            "unknown"
        );
        assert_eq!(
            playability_status(&serde_json::json!({"playabilityStatus":{"status":"UNPLAYABLE"}})),
            "UNPLAYABLE"
        );
    }
}

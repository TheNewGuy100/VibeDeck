//! Pure pieces for turning a `/player` audio format into a media URL for the byte-range UMP mode:
//! pick the format, split `signatureCipher`, collect the `n`/`s` challenges and assemble the final
//! URL once a solver has answered. No network, no provider text in errors, `pot` is never added.

use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub struct FormatParts {
    pub url: reqwest::Url,
    /// Scrambled signature from `signatureCipher`, when the format has no ready URL.
    pub s: Option<String>,
    /// Query name the deciphered signature goes under (`sp`, default `signature`).
    pub sp: String,
}

pub struct MediaParams<'a> {
    pub cver: &'a str,
    pub cpn: &'a str,
    pub rn: u32,
    pub range: (u64, u64),
    /// Milliseconds already buffered ahead (the player sends 0 on its first request).
    pub rbuf: u32,
}

/// Prefers AAC (itag 140), then any audio format that carries a URL or a cipher.
pub fn pick_audio(payload: &Value) -> Result<&Value, &'static str> {
    let formats = payload
        .pointer("/streamingData/adaptiveFormats")
        .and_then(|v| v.as_array())
        .ok_or("resolver_no_formats")?;
    let audio: Vec<&Value> = formats
        .iter()
        .filter(|f| {
            f.get("mimeType")
                .and_then(|m| m.as_str())
                .is_some_and(|m| m.starts_with("audio/"))
                && (f.get("url").is_some() || f.get("signatureCipher").is_some())
        })
        .collect();
    audio
        .iter()
        .copied()
        .find(|f| f.get("itag").and_then(|i| i.as_u64()) == Some(140))
        .or_else(|| audio.first().copied())
        .ok_or("resolver_no_audio_format")
}

pub fn parse_format(format: &Value) -> Result<FormatParts, &'static str> {
    if let Some(raw) = format.get("url").and_then(|u| u.as_str()) {
        let url = reqwest::Url::parse(raw).map_err(|_| "resolver_url_invalid")?;
        return Ok(FormatParts { url, s: None, sp: "signature".into() });
    }
    let cipher = format
        .get("signatureCipher")
        .or_else(|| format.get("cipher"))
        .and_then(|c| c.as_str())
        .ok_or("resolver_no_url_or_cipher")?;
    let (mut url, mut s, mut sp) = (None, None, None);
    for (key, value) in url::form_urlencoded::parse(cipher.as_bytes()) {
        match key.as_ref() {
            "url" => url = Some(value.into_owned()),
            "s" => s = Some(value.into_owned()),
            "sp" => sp = Some(value.into_owned()),
            _ => {}
        }
    }
    let url = reqwest::Url::parse(&url.ok_or("resolver_cipher_without_url")?)
        .map_err(|_| "resolver_url_invalid")?;
    let sp = sp.unwrap_or_else(|| "signature".into());
    if sp.is_empty() || !sp.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("resolver_sp_rejected");
    }
    Ok(FormatParts { url, s, sp })
}

impl FormatParts {
    pub fn host_ok(&self) -> bool {
        self.url.scheme() == "https"
            && self.url.host_str().is_some_and(|h| h.ends_with(".googlevideo.com"))
            && self.url.path() == "/videoplayback"
    }

    /// The raw `n` challenge in the URL, if any.
    pub fn n_challenge(&self) -> Option<String> {
        self.url.query_pairs().find(|(k, _)| k == "n").map(|(_, v)| v.into_owned())
    }
}

/// Final URL: transformed `n`, deciphered signature, player-style transport parameters, no `pot`.
pub fn assemble(
    parts: &FormatParts,
    n_out: Option<&str>,
    sig_out: Option<&str>,
    media: &MediaParams<'_>,
) -> Result<reqwest::Url, &'static str> {
    if !parts.host_ok() {
        return Err("resolver_url_rejected");
    }
    if parts.s.is_some() && sig_out.is_none() {
        return Err("resolver_signature_missing");
    }
    let mut pairs: Vec<(String, String)> = parts
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .filter(|(k, _)| !matches!(k.as_str(), "pot" | "alr" | "cpn" | "cver" | "range" | "rn" | "ump" | "rbuf" | "srfvp"))
        .collect();
    if let Some(n) = n_out {
        match pairs.iter_mut().find(|(k, _)| k == "n") {
            Some(slot) => slot.1 = n.to_owned(),
            None => return Err("resolver_n_missing_in_url"),
        }
    }
    if let Some(sig) = sig_out {
        pairs.retain(|(k, _)| *k != parts.sp);
        pairs.push((parts.sp.clone(), sig.to_owned()));
    }
    pairs.push(("alr".into(), "yes".into()));
    pairs.push(("cpn".into(), media.cpn.to_owned()));
    pairs.push(("cver".into(), media.cver.to_owned()));
    pairs.push(("range".into(), format!("{}-{}", media.range.0, media.range.1)));
    pairs.push(("rn".into(), media.rn.to_string()));
    pairs.push(("rbuf".into(), media.rbuf.to_string()));
    pairs.push(("ump".into(), "1".into()));
    pairs.push(("srfvp".into(), "1".into()));
    let mut url = parts.url.clone();
    url.query_pairs_mut().clear().extend_pairs(pairs);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BASE: &str = "https://rr1---sn-x.googlevideo.com/videoplayback?expire=1&itag=140&n=RAWN&pot=SECRET&range=0-1&mime=audio%2Fmp4";

    fn cipher(s: &str) -> Value {
        let url = url::form_urlencoded::byte_serialize(BASE.as_bytes()).collect::<String>();
        json!({"itag":140,"mimeType":"audio/mp4","signatureCipher": format!("s={s}&sp=sig&url={url}")})
    }

    fn media() -> MediaParams<'static> {
        MediaParams { cver: "1.2", cpn: "abcdefghijklmnop", rn: 7, range: (0, 65535), rbuf: 0 }
    }

    #[test]
    fn picks_aac_first_and_skips_formats_without_url_or_cipher() {
        let payload = json!({"streamingData":{"adaptiveFormats":[
            {"itag":251,"mimeType":"audio/webm","url":"https://x"},
            {"itag":139,"mimeType":"audio/mp4"},
            {"itag":140,"mimeType":"audio/mp4","signatureCipher":"s=1&url=2"},
            {"itag":137,"mimeType":"video/mp4","url":"https://v"}]}});
        assert_eq!(pick_audio(&payload).unwrap()["itag"], 140);
        let none = json!({"streamingData":{"adaptiveFormats":[{"itag":139,"mimeType":"audio/mp4"}]}});
        assert_eq!(pick_audio(&none).unwrap_err(), "resolver_no_audio_format");
    }

    #[test]
    fn cipher_is_split_into_url_signature_and_parameter_name() {
        let parts = parse_format(&cipher("AB%3DCD")).unwrap();
        assert_eq!(parts.s.as_deref(), Some("AB=CD"));
        assert_eq!(parts.sp, "sig");
        assert_eq!(parts.n_challenge().as_deref(), Some("RAWN"));
        assert!(parts.host_ok());
    }

    #[test]
    fn assembled_url_has_answers_transport_params_and_no_pot() {
        let parts = parse_format(&cipher("SCRAMBLED")).unwrap();
        let url = assemble(&parts, Some("NEWN"), Some("GOODSIG"), &media()).unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["n"], "NEWN");
        assert_eq!(pairs["sig"], "GOODSIG");
        assert_eq!((pairs["range"].as_str(), pairs["rn"].as_str(), pairs["ump"].as_str()), ("0-65535", "7", "1"));
        assert_eq!(pairs["alr"], "yes");
        assert_eq!((pairs["rbuf"].as_str(), pairs["srfvp"].as_str()), ("0", "1"));
        assert!(!pairs.contains_key("pot") && !url.as_str().contains("SECRET") && !url.as_str().contains("RAWN"));
        assert_eq!(url.query_pairs().filter(|(k, _)| k == "range").count(), 1);
    }

    #[test]
    fn missing_signature_or_foreign_host_is_refused() {
        let parts = parse_format(&cipher("S")).unwrap();
        assert_eq!(assemble(&parts, Some("N"), None, &media()).unwrap_err(), "resolver_signature_missing");
        let foreign = FormatParts {
            url: reqwest::Url::parse("https://evil.example/videoplayback?n=a").unwrap(),
            s: None,
            sp: "signature".into(),
        };
        assert_eq!(assemble(&foreign, None, None, &media()).unwrap_err(), "resolver_url_rejected");
    }

    #[test]
    fn direct_url_formats_need_no_signature() {
        let parts = parse_format(&json!({"url": BASE})).unwrap();
        assert!(parts.s.is_none());
        assert!(assemble(&parts, Some("N2"), None, &media()).is_ok());
    }
}

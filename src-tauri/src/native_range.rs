//! Native byte-range playback: resolves a track to a signed media URL with the app's own requests
//! (page, public player script, `/player`) and the yt-dlp-ejs solver; no `pot`, no cookies on media.
//! Solver logic: yt-dlp/ejs (Unlicense) bundling meriyah (ISC) and astring (MIT); see README.

use crate::sabr::url_resolver::{assemble, parse_format, pick_audio, MediaParams};
use vibedeck_decipher_eval::{ejs_solver, player_cache};

/// 16 characters from the URL-safe base64 alphabet; a client nonce, not a secret.
pub fn new_cpn() -> String {
    use std::hash::{BuildHasher, Hasher};
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    (0..16)
        .map(|index| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_usize(index);
            ALPHABET[(hasher.finish() % 64) as usize] as char
        })
        .collect()
}

const PREPARED_LIMIT: u64 = 16 * 1024 * 1024;
const PREPARED_SLOTS: usize = 3;

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaReport {
    pub stage: &'static str,
    pub error_code: Option<String>,
    pub prepared_cache_hit: bool,
    pub prepare_ms: Option<u128>,
    pub solve_ms: Option<u128>,
    pub script_cache_hit: bool,
    pub script_bytes: usize,
    pub auxiliary_gets: u8,
    pub sts_declared_by_player: bool,
    pub itag_is_140: bool,
    pub format_had_cipher: bool,
    pub n_in_len: Option<usize>,
    pub n_out_len: Option<usize>,
    pub sig_in_len: Option<usize>,
    pub sig_out_len: Option<usize>,
}

/// `signatureTimestamp:<digits>` as declared by the player script itself.
fn signature_timestamp(source: &str) -> Option<u32> {
    let at = source.find("signatureTimestamp:")? + "signatureTimestamp:".len();
    let digits: String = source[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 9 {
        return None;
    }
    digits.parse().ok()
}

fn local_dir(name: &str) -> Result<std::path::PathBuf, String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("native_media_localappdata_unset")?;
    Ok(std::path::PathBuf::from(local).join("VibeDeck").join(name))
}

/// Prepared script for this exact player source, cached on disk by content identity.
fn prepared_for(
    solver_dir: &std::path::Path,
    source: &str,
    report: &mut NativeMediaReport,
) -> Result<String, String> {
    let dir = local_dir("player-prepared")?;
    std::fs::create_dir_all(&dir).map_err(|_| "native_media_cache_dir_failed")?;
    let id = format!("{:016x}", player_cache::fnv64(source.as_bytes()));
    let path = dir.join(format!("{id}.js"));
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() <= PREPARED_LIMIT {
            if let Ok(text) = std::fs::read_to_string(&path) {
                report.prepared_cache_hit = true;
                return Ok(text);
            }
        }
    }
    let started = std::time::Instant::now();
    let prepared = ejs_solver::prepare_player(solver_dir, source)?;
    report.prepare_ms = Some(started.elapsed().as_millis());
    if prepared.len() as u64 > PREPARED_LIMIT {
        return Err("native_media_prepared_too_large".into());
    }
    let tmp = dir.join(format!("{id}.tmp"));
    std::fs::write(&tmp, &prepared).map_err(|_| "native_media_cache_write_failed")?;
    std::fs::rename(&tmp, &path).map_err(|_| "native_media_cache_write_failed")?;
    // Keep the newest few revisions only.
    if let Ok(read) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = read
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "js"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, old) in files.into_iter().skip(PREPARED_SLOTS) {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(prepared)
}

/// Everything a media URL needs, resolved natively: the chosen format, the answered challenges and
/// the client version. Shared by the single-request and the playback probes.
pub struct Resolved {
    pub parts: crate::sabr::url_resolver::FormatParts,
    pub n_answer: String,
    pub sig_answer: Option<String>,
    pub cver: String,
    pub format: serde_json::Value,
}

impl Resolved {
    pub fn url(&self, range: (u64, u64), rn: u32, rbuf: u32) -> Result<reqwest::Url, &'static str> {
        let cpn = new_cpn();
        assemble(
            &self.parts,
            Some(&self.n_answer),
            self.sig_answer.as_deref(),
            &MediaParams { cver: &self.cver, cpn: &cpn, rn, range, rbuf },
        )
    }
}

const SCRIPT_LIMIT: usize = 6 * 1024 * 1024;
const SCRIPT_TTL_SECS: u64 = 7 * 24 * 60 * 60;
const SCRIPT_SLOTS: usize = 3;

/// The public player script for the revision the music page currently declares. One GET of the
/// page (with the app's cookies, as the real page load) and, unless this revision is cached,
/// one cookie-less GET of the script itself. The declaration is read by the strict inspector, so
/// the path can only be `/s/player/<id>/<variant>/base.js` on music.youtube.com.
async fn acquire_player(
    app: &tauri::AppHandle,
    report: &mut NativeMediaReport,
) -> Result<String, String> {
    use tauri::Manager;
    let session_path = app
        .path()
        .app_data_dir()
        .map_err(|_| "native_media_directory_failed")?
        .join("youtube-session.bin");
    let session = crate::youtube_session::YouTubeSession::load(&session_path)
        .map_err(|_| "native_media_session_unavailable")?;
    let now = crate::unix_now().map_err(|_| "native_media_clock_failed")?;
    let cookie = session
        .cookie_header_for("https://music.youtube.com/", now)
        .map_err(|_| "native_media_session_ineligible")?;
    let http = crate::youtube_http::client()?;
    report.auxiliary_gets += 1;
    let response = http
        .get("https://music.youtube.com/")
        .header("Cookie", cookie)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131 Safari/537.36",
        )
        .send()
        .await
        .map_err(|e| crate::youtube_http::request_error(&e))?;
    let page = crate::youtube_http::bounded_body(response, 4 * 1024 * 1024).await?;
    let html = std::str::from_utf8(&page).map_err(|_| "native_media_page_encoding")?;
    let declaration = vibedeck_bootstrap_inspect::script_diagnostic::select_music_script(html)?;
    let path = declaration.path().to_string();
    let dir = local_dir("player-source")?;
    std::fs::create_dir_all(&dir).map_err(|_| "native_media_cache_dir_failed")?;
    let id = format!("{:016x}", player_cache::fnv64(path.as_bytes()));
    let file = dir.join(format!("{id}.js"));
    let fresh = std::fs::metadata(&file)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < SCRIPT_TTL_SECS);
    if fresh {
        if let Ok(text) = std::fs::read_to_string(&file) {
            report.script_cache_hit = true;
            report.script_bytes = text.len();
            return Ok(text);
        }
    }
    report.auxiliary_gets += 1;
    let response = http
        .get(format!("https://music.youtube.com{path}"))
        .send()
        .await
        .map_err(|e| crate::youtube_http::request_error(&e))?;
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !matches!(mime.as_str(), "application/javascript" | "text/javascript") {
        return Err("native_media_script_mime".into());
    }
    let bytes = crate::youtube_http::bounded_body(response, SCRIPT_LIMIT).await?;
    let text = String::from_utf8(bytes).map_err(|_| "native_media_script_encoding")?;
    if !text.contains("signatureTimestamp:") {
        return Err("native_media_script_unexpected".into());
    }
    report.script_bytes = text.len();
    let tmp = dir.join(format!("{id}.tmp"));
    std::fs::write(&tmp, &text).map_err(|_| "native_media_cache_write_failed")?;
    std::fs::rename(&tmp, &file).map_err(|_| "native_media_cache_write_failed")?;
    if let Ok(read) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = read
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "js"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, old) in files.into_iter().skip(SCRIPT_SLOTS) {
            let _ = std::fs::remove_file(old);
        }
    }
    Ok(text)
}

pub async fn resolve_audio(
    app: &tauri::AppHandle,
    video: &str,
    report: &mut NativeMediaReport,
) -> Result<Resolved, String> {
    let solver_dir = local_dir("ejs-solver")?;
    // 1. Player source (public script already in the local lab cache) and its prepared form.
    report.stage = "prepare";
    let source = acquire_player(app, report).await?;
    let sts = signature_timestamp(&source);
    report.sts_declared_by_player = sts.is_some();
    let prepared = {
        let dir = solver_dir.clone();
        let (result, local) = tauri::async_runtime::spawn_blocking(move || {
            let mut local = NativeMediaReport::default();
            let result = prepared_for(&dir, &source, &mut local);
            (result, local)
        })
        .await
        .map_err(|_| "native_media_prepare_join_failed")?;
        report.prepared_cache_hit = local.prepared_cache_hit;
        report.prepare_ms = local.prepare_ms;
        result?
    };
    // 2. The only metadata request: the app's own authenticated `/player`.
    report.stage = "player";
    report.auxiliary_gets += 1; // `fetch_youtube_player` loads the page once more
    let fetch = crate::fetch_youtube_player_with_sts(app, video, sts).await?;
    let format = pick_audio(&fetch.payload)?.clone();
    report.itag_is_140 = format.get("itag").and_then(|i| i.as_u64()) == Some(140);
    let parts = parse_format(&format)?;
    report.format_had_cipher = parts.s.is_some();
    // 3. Answer the challenges from the prepared script.
    report.stage = "solve";
    let n_challenge = parts.n_challenge().ok_or("native_media_n_missing")?;
    report.n_in_len = Some(n_challenge.len());
    report.sig_in_len = parts.s.as_ref().map(|s| s.len());
    let sigs: Vec<String> = parts.s.iter().cloned().collect();
    let started = std::time::Instant::now();
    let (dir, ns) = (solver_dir, vec![n_challenge.clone()]);
    let (n_out, sig_out) = tauri::async_runtime::spawn_blocking(move || {
        ejs_solver::solve_prepared(&dir, &prepared, &ns, &sigs)
    })
    .await
    .map_err(|_| "native_media_solve_join_failed")??;
    report.solve_ms = Some(started.elapsed().as_millis());
    let n_answer = n_out
        .into_iter()
        .find(|(input, _)| *input == n_challenge)
        .map(|(_, output)| output)
        .ok_or("native_media_n_unanswered")?;
    report.n_out_len = Some(n_answer.len());
    let sig_answer = match &parts.s {
        Some(s) => Some(
            sig_out
                .into_iter()
                .find(|(input, _)| input == s)
                .map(|(_, output)| output)
                .ok_or("native_media_sig_unanswered")?,
        ),
        None => None,
    };
    report.sig_out_len = sig_answer.as_ref().map(|s| s.len());
    Ok(Resolved { parts, n_answer, sig_answer, cver: fetch.client_version, format })
}


#[cfg(test)]
mod tests {
    use super::signature_timestamp;

    #[test]
    fn timestamp_is_read_from_the_script_and_bounded() {
        assert_eq!(signature_timestamp("x,signatureTimestamp:20759}y"), Some(20759));
        assert_eq!(signature_timestamp("signatureTimestamp:}"), None);
        assert_eq!(signature_timestamp("signatureTimestamp:12345678901"), None);
        assert_eq!(signature_timestamp("nothing here"), None);
    }
}

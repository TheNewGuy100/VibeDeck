//! Ephemeral local cache of a *public* player script, kept outside the repository so offline
//! analysis can iterate without going back to the browser each time.
//!
//! This is not a secret store and it does not encrypt: the player script is a public static
//! file and carries no credentials. It still never lives in the project tree, expires after
//! a day and holds only a few revisions. Never put cookies, tokens or API responses here.

use std::fs;
use std::path::{Path, PathBuf};

pub const TTL_SECS: u64 = 24 * 60 * 60;
pub const MAX_SLOTS: usize = 3;
pub const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub bytes: usize,
    pub ingested_unix: u64,
}

/// FNV-1a 64: identity of a public script for comparing revisions, not a security hash.
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn script_path(base: &Path, id: &str) -> PathBuf {
    base.join(format!("{id}.js"))
}

fn meta_path(base: &Path, id: &str) -> PathBuf {
    base.join(format!("{id}.meta"))
}

fn read_entry(base: &Path, id: &str) -> Option<Entry> {
    let ingested_unix = fs::read_to_string(meta_path(base, id))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let bytes = fs::metadata(script_path(base, id)).ok()?.len() as usize;
    Some(Entry {
        id: id.to_string(),
        bytes,
        ingested_unix,
    })
}

fn remove(base: &Path, id: &str) {
    let _ = fs::remove_file(script_path(base, id));
    let _ = fs::remove_file(meta_path(base, id));
}

fn ids(base: &Path) -> Vec<String> {
    let Ok(dir) = fs::read_dir(base) else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let id = name.strip_suffix(".js")?;
            valid_id(id).then(|| id.to_string())
        })
        .collect()
}

/// Entries still within the TTL, newest first. Expired or unreadable slots are removed.
pub fn list(base: &Path, now_unix: u64) -> Vec<Entry> {
    let mut entries = Vec::new();
    for id in ids(base) {
        match read_entry(base, &id) {
            Some(entry) if now_unix.saturating_sub(entry.ingested_unix) <= TTL_SECS => {
                entries.push(entry)
            }
            _ => remove(base, &id),
        }
    }
    entries.sort_by(|a, b| b.ingested_unix.cmp(&a.ingested_unix).then(a.id.cmp(&b.id)));
    entries
}

pub fn ingest(base: &Path, source: &str, now_unix: u64) -> Result<Entry, &'static str> {
    let source = source.trim_start_matches('\u{feff}');
    if source.is_empty() {
        return Err("cache_source_empty");
    }
    if source.len() > MAX_BYTES {
        return Err("cache_source_too_large");
    }
    fs::create_dir_all(base).map_err(|_| "cache_dir_unavailable")?;
    let _ = list(base, now_unix); // drops expired slots first
    let id = format!("{:016x}", fnv64(source.as_bytes()));
    fs::write(script_path(base, &id), source).map_err(|_| "cache_write_failed")?;
    fs::write(meta_path(base, &id), format!("{now_unix}\n")).map_err(|_| "cache_write_failed")?;
    for stale in list(base, now_unix).into_iter().skip(MAX_SLOTS) {
        remove(base, &stale.id);
    }
    read_entry(base, &id).ok_or("cache_write_failed")
}

/// Latest entry when `id` is `None`.
pub fn load(base: &Path, id: Option<&str>, now_unix: u64) -> Result<(Entry, String), &'static str> {
    let entries = list(base, now_unix);
    let entry = match id {
        Some(id) if !valid_id(id) => return Err("cache_id_invalid"),
        Some(id) => entries
            .into_iter()
            .find(|e| e.id == id)
            .ok_or("cache_miss")?,
        None => entries.into_iter().next().ok_or("cache_empty")?,
    };
    let source =
        fs::read_to_string(script_path(base, &entry.id)).map_err(|_| "cache_read_failed")?;
    Ok((entry, source))
}

pub fn purge_all(base: &Path) -> usize {
    let found = ids(base);
    for id in &found {
        remove(base, id);
    }
    found.len()
}

/// `%LOCALAPPDATA%\VibeDeck\player-lab`, refused when it would sit inside `project_root`.
pub fn default_base(project_root: &Path) -> Result<PathBuf, &'static str> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("cache_localappdata_unset")?;
    let base = PathBuf::from(local).join("VibeDeck").join("player-lab");
    ensure_outside(&base, project_root)?;
    Ok(base)
}

pub fn ensure_outside(base: &Path, project_root: &Path) -> Result<(), &'static str> {
    let absolute = |p: &Path| std::path::absolute(p).map_err(|_| "cache_path_invalid");
    let (base, root) = (absolute(base)?, absolute(project_root)?);
    if base.starts_with(&root) {
        return Err("cache_inside_project");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_base(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "vibedeck-player-cache-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        base
    }

    #[test]
    fn ingest_load_and_identity_by_content() {
        let base = temp_base("basic");
        let a = ingest(&base, "var a=1;", 1_000).unwrap();
        assert_eq!(a.bytes, 8);
        assert_eq!(a.id, format!("{:016x}", fnv64(b"var a=1;")));
        let same = ingest(&base, "\u{feff}var a=1;", 1_005).unwrap();
        assert_eq!(same.id, a.id, "BOM must not change identity");
        let (entry, source) = load(&base, None, 1_010).unwrap();
        assert_eq!((entry.id, source.as_str()), (a.id.clone(), "var a=1;"));
        assert_eq!(load(&base, Some(&a.id), 1_010).unwrap().1, "var a=1;");
        assert_eq!(
            load(&base, Some("deadbeefdeadbeef"), 1_010).unwrap_err(),
            "cache_miss"
        );
        assert_eq!(
            load(&base, Some("../../etc/passwd"), 1_010).unwrap_err(),
            "cache_id_invalid"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn entries_expire_and_slots_are_bounded() {
        let base = temp_base("ttl");
        for (i, src) in ["var a;", "var b;", "var c;", "var d;"].iter().enumerate() {
            ingest(&base, src, 100 + i as u64).unwrap();
        }
        let listed = list(&base, 110);
        assert_eq!(listed.len(), MAX_SLOTS);
        assert_eq!(listed[0].ingested_unix, 103, "newest first; oldest evicted");
        assert!(
            list(&base, 103 + TTL_SECS).len() == 1,
            "older ones expire by the TTL"
        );
        assert!(list(&base, 103 + TTL_SECS + 1).is_empty());
        assert_eq!(
            load(&base, None, 103 + TTL_SECS + 1).unwrap_err(),
            "cache_empty"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_empty_oversized_and_project_paths_and_purges() {
        let base = temp_base("limits");
        assert_eq!(ingest(&base, "", 1).unwrap_err(), "cache_source_empty");
        assert_eq!(
            ingest(&base, "\u{feff}", 1).unwrap_err(),
            "cache_source_empty"
        );
        assert_eq!(
            ingest(&base, &"x".repeat(MAX_BYTES + 1), 1).unwrap_err(),
            "cache_source_too_large"
        );
        ingest(&base, "var x;", 1).unwrap();
        ingest(&base, "var y;", 2).unwrap();
        assert_eq!(purge_all(&base), 2);
        assert!(list(&base, 3).is_empty());
        let project = std::env::temp_dir().join("vibedeck-project-root");
        assert_eq!(
            ensure_outside(&project.join("cache"), &project).unwrap_err(),
            "cache_inside_project"
        );
        assert!(ensure_outside(&std::env::temp_dir().join("elsewhere"), &project).is_ok());
        let _ = fs::remove_dir_all(&base);
    }
}

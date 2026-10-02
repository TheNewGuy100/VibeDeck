use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};

pub const CATALOG_VERSION: u32 = 1;

static CATALOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistCatalog {
    pub version: u32,
    pub playlists: Vec<PlaylistEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub id: String,
    pub name: String,
    pub source: String,
    pub mode: String,
    pub allow_local: bool,
    pub generation: u64,
    pub tracks: Vec<Value>,
    pub continuation: Option<String>,
    pub terminal: bool,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePlaylistPageRequest {
    pub playlist_id: String,
    pub name: String,
    pub source: String,
    pub mode: String,
    pub allow_local: bool,
    pub generation: u64,
    pub tracks: Vec<Value>,
    pub continuation: Option<String>,
    pub has_more: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistCatalogSnapshot {
    pub version: u32,
    pub playlists: Vec<PlaylistEntry>,
}

pub fn catalog_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("playlist-catalog.json"))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn load_playlist_catalog(app: AppHandle) -> Result<PlaylistCatalogSnapshot, String> {
    let path = catalog_path(&app)?;
    let catalog = load_from_path(&path)?;
    Ok(snapshot(&catalog))
}

pub fn continuation_for_playlist(
    app: &AppHandle,
    playlist_id: &str,
) -> Result<Option<String>, String> {
    let path = catalog_path(app)?;
    Ok(load_from_path(&path)?
        .playlists
        .into_iter()
        .find(|playlist| playlist.id == playlist_id && !playlist.terminal)
        .and_then(|playlist| playlist.continuation))
}

#[tauri::command]
pub fn save_youtube_playlist_page(
    app: AppHandle,
    request: SavePlaylistPageRequest,
) -> Result<PlaylistCatalogSnapshot, String> {
    if request.playlist_id.trim().is_empty() || request.generation == 0 {
        return Err("playlistId and generation are required".to_string());
    }
    let _guard = CATALOG_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Playlist catalog lock poisoned".to_string())?;
    let path = catalog_path(&app)?;
    let mut catalog = load_from_path(&path)?;
    merge_page(&mut catalog, request)?;
    save_to_path(&path, &catalog)?;
    Ok(snapshot(&catalog))
}

fn snapshot(catalog: &PlaylistCatalog) -> PlaylistCatalogSnapshot {
    PlaylistCatalogSnapshot {
        version: catalog.version,
        playlists: catalog.playlists.clone(),
    }
}

fn merge_page(
    catalog: &mut PlaylistCatalog,
    request: SavePlaylistPageRequest,
) -> Result<(), String> {
    if catalog.version != CATALOG_VERSION {
        *catalog = PlaylistCatalog {
            version: CATALOG_VERSION,
            playlists: Vec::new(),
        };
    }
    let index = catalog
        .playlists
        .iter()
        .position(|item| item.id == request.playlist_id);
    let entry = if let Some(index) = index {
        let entry = &mut catalog.playlists[index];
        if request.generation < entry.generation {
            return Ok(());
        }
        if request.generation > entry.generation {
            entry.generation = request.generation;
            entry.last_error = None;
        }
        entry
    } else {
        catalog.playlists.push(PlaylistEntry {
            id: request.playlist_id.clone(),
            name: request.name.clone(),
            source: request.source.clone(),
            mode: request.mode.clone(),
            allow_local: request.allow_local,
            generation: request.generation,
            tracks: Vec::new(),
            continuation: None,
            terminal: false,
            last_error: None,
        });
        catalog
            .playlists
            .last_mut()
            .expect("playlist was just inserted")
    };
    entry.name = request.name;
    entry.source = request.source;
    entry.mode = request.mode;
    entry.allow_local = request.allow_local;
    for track in request.tracks {
        let track = sanitize_track(track);
        let Some(id) = track.get("id").and_then(Value::as_str) else {
            continue;
        };
        if let Some(existing) = entry
            .tracks
            .iter_mut()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
        {
            *existing = track;
        } else {
            entry.tracks.push(track);
        }
    }
    if request.has_more {
        if let Some(continuation) = request
            .continuation
            .filter(|value| !value.trim().is_empty())
        {
            entry.continuation = Some(continuation);
        }
    } else {
        entry.continuation = None;
    }
    entry.terminal = !request.has_more;
    entry.last_error = request.error.map(|value| value.chars().take(240).collect());
    Ok(())
}

fn sanitize_track(track: Value) -> Value {
    let Some(object) = track.as_object() else {
        return Value::Null;
    };
    let allowed = [
        "id",
        "title",
        "artist",
        "album",
        "duration",
        "source",
        "accent",
        "coverUrl",
        "durationSeconds",
        "youtubeId",
        "managedPath",
        "fileName",
        "available",
    ];
    let mut clean = Map::new();
    let source = object.get("source").and_then(Value::as_str);
    for key in allowed {
        if let Some(value) = object.get(key) {
            if key == "coverUrl" && value.as_str().is_some_and(is_unsafe_url) {
                continue;
            }
            clean.insert(key.to_string(), value.clone());
        }
    }
    if source == Some("youtube") {
        clean.remove("managedPath");
        clean.remove("fileName");
        clean.remove("available");
    }
    Value::Object(clean)
}

fn is_unsafe_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("blob:")
        || lower.starts_with("file:")
        || lower.contains("videoplayback")
        || lower.contains("googlevideo.com")
        || lower.contains("signaturecipher")
        || lower.contains("token=")
        || lower.contains("cookie")
}

fn load_from_path(path: &Path) -> Result<PlaylistCatalog, String> {
    let backup = path.with_extension("json.bak");
    if !path.exists() {
        return read_catalog(&backup).or_else(|_| {
            Ok(PlaylistCatalog {
                version: CATALOG_VERSION,
                playlists: Vec::new(),
            })
        });
    }
    match read_catalog(path) {
        Ok(mut catalog) => {
            if catalog.version != CATALOG_VERSION {
                catalog.version = CATALOG_VERSION;
            }
            Ok(catalog)
        }
        Err(_) => {
            let _ = fs::rename(
                path,
                path.with_extension(format!("json.corrupt-{}", unique_suffix())),
            );
            read_catalog(&backup).or_else(|_| {
                Ok(PlaylistCatalog {
                    version: CATALOG_VERSION,
                    playlists: Vec::new(),
                })
            })
        }
    }
}

fn read_catalog(path: &Path) -> Result<PlaylistCatalog, String> {
    let mut contents = String::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .read_to_string(&mut contents)
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&contents).map_err(|error| error.to_string())
}

fn save_to_path(path: &Path, catalog: &PlaylistCatalog) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    let backup = path.with_extension("json.bak");
    let payload = serde_json::to_vec_pretty(catalog).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&payload)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    if path.exists() {
        let _ = fs::copy(path, &backup);
    }
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn unique_suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        generation: u64,
        tracks: &[(&str, &str)],
        has_more: bool,
    ) -> SavePlaylistPageRequest {
        SavePlaylistPageRequest { playlist_id: "VLPL1".into(), name: "Mix".into(), source: "online".into(), mode: "online".into(), allow_local: false, generation, tracks: tracks.iter().map(|(id, title)| serde_json::json!({"id": id, "title": title, "audioUrl": "blob:secret", "coverUrl": "https://img.youtube.com/x"})).collect(), continuation: has_more.then(|| "next".into()), has_more, error: None }
    }

    #[test]
    fn merges_pages_and_discards_live_fields() {
        let mut catalog = PlaylistCatalog {
            version: CATALOG_VERSION,
            playlists: vec![],
        };
        merge_page(&mut catalog, request(1, &[("a", "A"), ("b", "B")], true)).unwrap();
        merge_page(&mut catalog, request(1, &[("b", "B2"), ("c", "C")], false)).unwrap();
        assert_eq!(catalog.playlists[0].tracks.len(), 3);
        assert_eq!(catalog.playlists[0].tracks[1]["title"], "B2");
        assert!(catalog.playlists[0].tracks[0].get("audioUrl").is_none());
        assert!(catalog.playlists[0].terminal);
    }

    #[test]
    fn stale_generation_does_not_replace_current_queue() {
        let mut catalog = PlaylistCatalog {
            version: CATALOG_VERSION,
            playlists: vec![],
        };
        merge_page(&mut catalog, request(3, &[("new", "New")], false)).unwrap();
        merge_page(&mut catalog, request(2, &[("old", "Old")], false)).unwrap();
        assert_eq!(catalog.playlists[0].generation, 3);
        assert_eq!(catalog.playlists[0].tracks[0]["id"], "new");
    }

    #[test]
    fn atomic_catalog_round_trip_and_backup_recovery() {
        let root = std::env::temp_dir().join(format!("vibedeck-playlist-{}", unique_suffix()));
        let path = root.join("playlist-catalog.json");
        let mut catalog = PlaylistCatalog {
            version: CATALOG_VERSION,
            playlists: vec![],
        };
        merge_page(&mut catalog, request(1, &[("a", "A")], false)).unwrap();
        save_to_path(&path, &catalog).unwrap();
        merge_page(&mut catalog, request(2, &[("b", "B")], false)).unwrap();
        save_to_path(&path, &catalog).unwrap();
        fs::write(&path, b"broken").unwrap();
        assert_eq!(load_from_path(&path).unwrap().playlists[0].generation, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn merges_250_tracks_and_preserves_them_after_save_load() {
        let root =
            std::env::temp_dir().join(format!("vibedeck-playlist-pages-{}", unique_suffix()));
        let path = root.join("playlist-catalog.json");
        let mut catalog = PlaylistCatalog {
            version: CATALOG_VERSION,
            playlists: vec![],
        };
        for (page, tracks) in [(0, 100), (100, 100), (200, 50)] {
            let has_more = page + tracks < 250;
            merge_page(
                &mut catalog,
                SavePlaylistPageRequest {
                    playlist_id: "VLPL-pages".into(),
                    name: "Pages".into(),
                    source: "online".into(),
                    mode: "online".into(),
                    allow_local: false,
                    generation: 4,
                    tracks: (page..page + tracks)
                        .map(|index| serde_json::json!({ "id": format!("track-{index}") }))
                        .collect(),
                    continuation: has_more.then(|| format!("cursor-{}", page + tracks)),
                    has_more,
                    error: None,
                },
            )
            .unwrap();
        }
        save_to_path(&path, &catalog).unwrap();
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.playlists[0].tracks.len(), 250);
        assert!(loaded.playlists[0].terminal);
        assert_eq!(loaded.playlists[0].continuation, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persists_629_tracks_cursor_and_resumes_without_losing_prior_pages() {
        let root = std::env::temp_dir().join(format!("vibedeck-629-{}", unique_suffix()));
        let path = root.join("playlist-catalog.json");
        let mut catalog = PlaylistCatalog {
            version: CATALOG_VERSION,
            playlists: vec![],
        };
        for start in (0..629).step_by(100) {
            let end = (start + 100).min(629);
            merge_page(
                &mut catalog,
                SavePlaylistPageRequest {
                    playlist_id: "VLPL-best".into(),
                    name: "Just The Best".into(),
                    source: "online".into(),
                    mode: "online".into(),
                    allow_local: false,
                    generation: 1,
                    tracks: (start..end)
                        .map(|index| serde_json::json!({"id": format!("track-{index}")}))
                        .collect(),
                    continuation: (end < 629).then(|| format!("cursor-{end}")),
                    has_more: end < 629,
                    error: None,
                },
            )
            .unwrap();
            if end == 300 {
                save_to_path(&path, &catalog).unwrap();
                catalog = load_from_path(&path).unwrap();
            }
        }
        save_to_path(&path, &catalog).unwrap();
        let loaded = load_from_path(&path).unwrap();
        let playlist = &loaded.playlists[0];
        assert_eq!(playlist.tracks.len(), 629);
        assert!(playlist.terminal);
        assert_eq!(playlist.continuation, None);
        let _ = fs::remove_dir_all(root);
    }
}

//! Global favourites: one flag per track, stored as a reference (source + id) plus the minimum
//! metadata needed to list it without loading its playlist. Global is every item; Local and each
//! provider are filters over the same list. Favouriting never writes to a provider account.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

const FAVORITES_VERSION: u32 = 1;
const SOURCES: [&str; 2] = ["local", "youtube"];
const MAX_ID_CHARS: usize = 200;
const MAX_TEXT_CHARS: usize = 300;
const MAX_URL_CHARS: usize = 2048;

static FAVORITES_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteItem {
    pub source: String,
    pub id: String,
    #[serde(default)]
    pub added_at: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub youtube_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FavoritesStore {
    pub version: u32,
    /// Newest first.
    pub items: Vec<FavoriteItem>,
}

impl Default for FavoritesStore {
    fn default() -> Self {
        Self {
            version: FAVORITES_VERSION,
            items: Vec::new(),
        }
    }
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn sanitize(mut item: FavoriteItem, now: u64) -> Result<FavoriteItem, String> {
    if !SOURCES.contains(&item.source.as_str()) {
        return Err("Fonte de favorito desconhecida.".to_string());
    }
    item.id = item.id.trim().to_string();
    if item.id.is_empty() || item.id.chars().count() > MAX_ID_CHARS {
        return Err("Identificador de favorito inválido.".to_string());
    }
    item.title = truncate(item.title.trim(), MAX_TEXT_CHARS);
    item.artist = truncate(item.artist.trim(), MAX_TEXT_CHARS);
    item.album = truncate(item.album.trim(), MAX_TEXT_CHARS);
    item.duration = truncate(item.duration.trim(), 16);
    item.cover_url = item
        .cover_url
        .filter(|url| url.len() <= MAX_URL_CHARS && url.starts_with("https://"));
    item.youtube_id = item
        .youtube_id
        .map(|value| truncate(value.trim(), MAX_ID_CHARS))
        .filter(|value| !value.is_empty());
    item.added_at = now;
    Ok(item)
}

impl FavoritesStore {
    pub fn contains(&self, source: &str, id: &str) -> bool {
        self.items
            .iter()
            .any(|item| item.source == source && item.id == id)
    }

    /// Adds (newest first, keeping the original date if already there) or removes one favourite.
    /// Returns whether the list changed.
    pub fn set(&mut self, item: FavoriteItem, favorite: bool, now: u64) -> Result<bool, String> {
        if !favorite {
            let before = self.items.len();
            self.items
                .retain(|existing| !(existing.source == item.source && existing.id == item.id));
            return Ok(self.items.len() != before);
        }
        let item = sanitize(item, now)?;
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|existing| existing.source == item.source && existing.id == item.id)
        {
            // Refresh the metadata but keep when it was favourited.
            let added_at = existing.added_at;
            *existing = FavoriteItem { added_at, ..item };
            return Ok(false);
        }
        self.items.insert(0, item);
        Ok(true)
    }

    pub fn ids_for(&self, source: &str) -> HashSet<String> {
        self.items
            .iter()
            .filter(|item| item.source == source)
            .map(|item| item.id.clone())
            .collect()
    }
}

fn favorites_path(root: &Path) -> PathBuf {
    root.join("favorites.json")
}

fn load_from(root: &Path) -> Result<FavoritesStore, String> {
    let path = favorites_path(root);
    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<FavoritesStore>(&raw) {
            Ok(store) if store.version == FAVORITES_VERSION => Ok(store),
            Ok(_) | Err(_) => {
                // Keep the unreadable file so nothing is lost, and refuse to continue silently.
                let backup = path.with_file_name(format!(
                    "favorites.json.corrupt-{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|value| value.as_nanos())
                        .unwrap_or_default()
                ));
                let _ = fs::rename(&path, backup);
                Err("A lista de favoritos estava ilegível e foi preservada.".to_string())
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(FavoritesStore::default()),
        Err(error) => Err(error.to_string()),
    }
}

fn save_to(root: &Path, store: &FavoritesStore) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let path = favorites_path(root);
    let temp = root.join(format!(
        "favorites.json.{}.tmp",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or_default()
    ));
    let bytes = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    fs::rename(temp, path).map_err(|error| error.to_string())
}

/// Ids favourited for one source. Fails when the file is unreadable so callers that delete files
/// (the orphan purge) never act on a guess.
pub fn favorite_ids(root: &Path, source: &str) -> Result<HashSet<String>, String> {
    Ok(load_from(root)?.ids_for(source))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    FAVORITES_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Favorites lock poisoned".to_string())
}

#[tauri::command]
pub fn load_favorites(app: AppHandle) -> Result<FavoritesStore, String> {
    let _guard = lock()?;
    load_from(&crate::local_library::library_root(&app)?)
}

#[tauri::command]
pub fn set_favorite(
    app: AppHandle,
    item: FavoriteItem,
    favorite: bool,
) -> Result<FavoritesStore, String> {
    let _guard = lock()?;
    let root = crate::local_library::library_root(&app)?;
    let mut store = load_from(&root)?;
    if store.set(item, favorite, now())? {
        save_to(&root, &store)?;
    }
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(source: &str, id: &str) -> FavoriteItem {
        FavoriteItem {
            source: source.to_string(),
            id: id.to_string(),
            title: format!("Title {id}"),
            ..FavoriteItem::default()
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "vibedeck-fav-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn favouriting_is_idempotent_and_newest_first() {
        let mut store = FavoritesStore::default();
        assert!(store.set(item("local", "a"), true, 10).unwrap());
        assert!(store.set(item("youtube", "b"), true, 20).unwrap());
        assert!(!store.set(item("local", "a"), true, 30).unwrap());
        assert_eq!(store.items.len(), 2);
        assert_eq!(store.items[0].id, "b");
        assert_eq!(store.items[1].added_at, 10, "keeps the original date");
    }

    #[test]
    fn the_same_id_in_two_sources_is_two_favourites() {
        let mut store = FavoritesStore::default();
        store.set(item("local", "x"), true, 1).unwrap();
        store.set(item("youtube", "x"), true, 1).unwrap();
        assert_eq!(store.items.len(), 2);
        assert!(store.contains("local", "x") && store.contains("youtube", "x"));
        assert!(store.set(item("local", "x"), false, 2).unwrap());
        assert!(!store.contains("local", "x") && store.contains("youtube", "x"));
        assert!(!store.set(item("local", "x"), false, 3).unwrap());
    }

    #[test]
    fn rejects_unknown_sources_and_bad_ids() {
        let mut store = FavoritesStore::default();
        assert!(store.set(item("spotify", "a"), true, 1).is_err());
        assert!(store.set(item("local", "  "), true, 1).is_err());
        assert!(store.set(item("local", &"x".repeat(201)), true, 1).is_err());
        assert!(store.items.is_empty());
    }

    #[test]
    fn sanitises_metadata() {
        let mut store = FavoritesStore::default();
        let mut long = item("youtube", "a");
        long.title = "t".repeat(500);
        long.cover_url = Some("http://insecure.example/x.jpg".to_string());
        long.youtube_id = Some("   ".to_string());
        store.set(long, true, 1).unwrap();
        assert_eq!(store.items[0].title.chars().count(), 300);
        assert!(store.items[0].cover_url.is_none());
        assert!(store.items[0].youtube_id.is_none());
    }

    #[test]
    fn persists_and_filters_ids_by_source() {
        let root = temp_root("persist");
        let mut store = FavoritesStore::default();
        store.set(item("local", "a"), true, 1).unwrap();
        store.set(item("youtube", "b"), true, 1).unwrap();
        save_to(&root, &store).unwrap();
        assert_eq!(load_from(&root).unwrap(), store);
        assert_eq!(favorite_ids(&root, "local").unwrap(), HashSet::from(["a".to_string()]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_file_is_empty_but_a_corrupt_one_is_an_error_and_preserved() {
        let root = temp_root("corrupt");
        assert!(favorite_ids(&root, "local").unwrap().is_empty());
        fs::write(favorites_path(&root), b"{ not json").unwrap();
        assert!(favorite_ids(&root, "local").is_err());
        assert!(!favorites_path(&root).exists());
        assert!(fs::read_dir(&root)
            .unwrap()
            .any(|entry| entry.unwrap().file_name().to_string_lossy().contains("corrupt")));
        fs::remove_dir_all(root).unwrap();
    }
}

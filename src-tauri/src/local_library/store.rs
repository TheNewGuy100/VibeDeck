//! Catalog v2: audio files are content-addressed blobs (`blobs/<hash>.<ext>`), and libraries are
//! lists of track ids. A track can sit in several libraries; a blob nobody references is an orphan
//! that is only deleted by an explicit, confirmed purge.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use super::{hex, is_safe_component, is_safe_relative, LocalLibraryCatalog, LocalPlaylist, LocalTrack};

pub const CATALOG_VERSION: u32 = 2;
const LEGACY_CATALOG_VERSION: u32 = 1;
const MAX_NAME_CHARS: usize = 80;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStore {
    pub version: u32,
    pub tracks: Vec<LocalTrack>,
    pub libraries: Vec<LocalLibrary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalLibrary {
    pub id: String,
    pub name: String,
    pub track_ids: Vec<String>,
    #[serde(default)]
    pub created_at: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddOutcome {
    /// The file is new to the catalog.
    AddedNew,
    /// The file already existed in another library; only a reference was added.
    AddedExisting,
    AlreadyInLibrary,
}

#[derive(Deserialize)]
struct LegacyCatalog {
    version: u32,
    playlists: Vec<LegacyPlaylist>,
}

#[derive(Deserialize)]
struct LegacyPlaylist {
    id: String,
    name: String,
    tracks: Vec<LocalTrack>,
}

pub fn stable_library_id(name: &str) -> String {
    let normalized = name
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut hash = Sha1::new();
    hash.update(normalized.as_bytes());
    format!("local-{}", &hex(&hash.finalize())[..16])
}

pub fn blob_relative(hash: &str, extension: &str) -> String {
    format!("blobs/{hash}.{extension}")
}

pub fn normalize_name(name: &str) -> Result<String, String> {
    let normalized = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("O nome da biblioteca é obrigatório.".to_string());
    }
    if normalized.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "O nome da biblioteca pode ter no máximo {MAX_NAME_CHARS} caracteres."
        ));
    }
    Ok(normalized)
}

impl CatalogStore {
    pub fn empty() -> Self {
        Self {
            version: CATALOG_VERSION,
            tracks: Vec::new(),
            libraries: Vec::new(),
        }
    }

    /// The shape the interface already consumes: libraries with their tracks inlined.
    pub fn view(&self) -> LocalLibraryCatalog {
        LocalLibraryCatalog {
            version: CATALOG_VERSION,
            playlists: self
                .libraries
                .iter()
                .map(|library| LocalPlaylist {
                    id: library.id.clone(),
                    name: library.name.clone(),
                    tracks: library
                        .track_ids
                        .iter()
                        .filter_map(|id| self.track(id).cloned())
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn track(&self, id: &str) -> Option<&LocalTrack> {
        self.tracks.iter().find(|track| track.id == id)
    }

    pub fn library_index(&self, id: &str) -> Option<usize> {
        self.libraries.iter().position(|library| library.id == id)
    }

    fn name_taken(&self, name: &str, except_id: Option<&str>) -> bool {
        let wanted = name.to_lowercase();
        self.libraries.iter().any(|library| {
            Some(library.id.as_str()) != except_id && library.name.to_lowercase() == wanted
        })
    }

    fn unique_id(&self, name: &str) -> String {
        let base = stable_library_id(name);
        if self.library_index(&base).is_none() {
            return base;
        }
        (2..)
            .map(|n| format!("{base}-{n}"))
            .find(|candidate| self.library_index(candidate).is_none())
            .expect("an unused suffix always exists")
    }

    pub fn create_library(&mut self, name: &str, now: u64) -> Result<usize, String> {
        let name = normalize_name(name)?;
        if self.name_taken(&name, None) {
            return Err("Já existe uma biblioteca com este nome.".to_string());
        }
        let id = self.unique_id(&name);
        self.libraries.push(LocalLibrary {
            id,
            name,
            track_ids: Vec::new(),
            created_at: now,
        });
        Ok(self.libraries.len() - 1)
    }

    /// Imports by folder name reuse a library with the same name (case-insensitive).
    pub fn find_or_create_library(&mut self, name: &str, now: u64) -> Result<usize, String> {
        let normalized = normalize_name(name)?;
        let wanted = normalized.to_lowercase();
        if let Some(index) = self
            .libraries
            .iter()
            .position(|library| library.name.to_lowercase() == wanted)
        {
            return Ok(index);
        }
        self.create_library(&normalized, now)
    }

    pub fn rename_library(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = normalize_name(name)?;
        let index = self
            .library_index(id)
            .ok_or_else(|| "Biblioteca não encontrada.".to_string())?;
        if self.name_taken(&name, Some(id)) {
            return Err("Já existe uma biblioteca com este nome.".to_string());
        }
        self.libraries[index].name = name;
        Ok(())
    }

    /// Removes the library and its references only. Files stay until an orphan purge.
    pub fn delete_library(&mut self, id: &str) -> bool {
        let before = self.libraries.len();
        self.libraries.retain(|library| library.id != id);
        self.libraries.len() != before
    }

    pub fn add_track(&mut self, library: usize, track: LocalTrack) -> AddOutcome {
        if self.libraries[library].track_ids.contains(&track.id) {
            return AddOutcome::AlreadyInLibrary;
        }
        let existing = self.tracks.iter().any(|item| item.id == track.id);
        let id = track.id.clone();
        if !existing {
            self.tracks.push(track);
        }
        self.libraries[library].track_ids.push(id);
        if existing {
            AddOutcome::AddedExisting
        } else {
            AddOutcome::AddedNew
        }
    }

    /// Takes the track out of one library. The file is kept (it may be an orphan afterwards).
    pub fn remove_track_ref(&mut self, library_id: &str, track_id: &str) -> bool {
        let Some(index) = self.library_index(library_id) else {
            return false;
        };
        let ids = &mut self.libraries[index].track_ids;
        let before = ids.len();
        ids.retain(|id| id != track_id);
        ids.len() != before
    }

    pub fn move_tracks(
        &mut self,
        from_id: &str,
        to_id: &str,
        track_ids: &[String],
    ) -> Result<usize, String> {
        let from = self
            .library_index(from_id)
            .ok_or_else(|| "Biblioteca de origem não encontrada.".to_string())?;
        let to = self
            .library_index(to_id)
            .ok_or_else(|| "Biblioteca de destino não encontrada.".to_string())?;
        if from == to {
            return Ok(0);
        }
        let mut moved = 0;
        for id in track_ids {
            if !self.libraries[from].track_ids.contains(id) {
                continue;
            }
            self.libraries[from].track_ids.retain(|item| item != id);
            if !self.libraries[to].track_ids.contains(id) {
                self.libraries[to].track_ids.push(id.clone());
            }
            moved += 1;
        }
        Ok(moved)
    }

    /// Tracks that no library lists and that are not in `protected` (favourites).
    pub fn orphans(&self, protected: &HashSet<String>) -> Vec<&LocalTrack> {
        let used: HashSet<&str> = self
            .libraries
            .iter()
            .flat_map(|library| library.track_ids.iter().map(String::as_str))
            .collect();
        self.tracks
            .iter()
            .filter(|track| !used.contains(track.id.as_str()) && !protected.contains(&track.id))
            .collect()
    }

    pub fn remove_tracks(&mut self, ids: &HashSet<String>) -> Vec<LocalTrack> {
        let (removed, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.tracks)
            .into_iter()
            .partition(|track| ids.contains(&track.id));
        self.tracks = kept;
        removed
    }
}

/// Converts a v1 catalog (files under `playlists/<id>/`) into v2, moving each file into `blobs/`.
/// Safe to repeat after a crash: a file already moved is detected and only the catalog is rebuilt.
pub fn migrate_legacy(root: &Path, raw: &str) -> Result<CatalogStore, String> {
    let legacy: LegacyCatalog = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    if legacy.version != LEGACY_CATALOG_VERSION {
        return Err("unsupported catalog version".to_string());
    }
    fs::create_dir_all(root.join("blobs")).map_err(|error| error.to_string())?;
    let mut store = CatalogStore::empty();
    for playlist in legacy.playlists {
        let mut ids: Vec<String> = Vec::new();
        for mut track in playlist.tracks {
            if !is_safe_component(&track.content_hash) || !is_safe_component(&track.extension) {
                return Err("Catalog contains an unsafe track reference".to_string());
            }
            let relative = blob_relative(&track.content_hash, &track.extension);
            let old = root.join(&track.managed_path);
            let new = root.join(&relative);
            if !new.exists() && is_safe_relative(&old, root) && old.is_file() {
                fs::rename(&old, &new).map_err(|error| error.to_string())?;
            }
            track.managed_path = relative;
            if !ids.contains(&track.id) {
                ids.push(track.id.clone());
            }
            if store.track(&track.id).is_none() {
                store.tracks.push(track);
            }
        }
        if is_safe_component(&playlist.id) {
            let _ = fs::remove_dir(root.join("playlists").join(&playlist.id));
        }
        store.libraries.push(LocalLibrary {
            id: playlist.id,
            name: playlist.name,
            track_ids: ids,
            created_at: 0,
        });
    }
    let _ = fs::remove_dir(root.join("playlists"));
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(hash: &str) -> LocalTrack {
        LocalTrack {
            id: format!("local-{hash}"),
            content_hash: hash.to_string(),
            byte_length: 4,
            extension: "mp3".to_string(),
            file_name: format!("{hash}.mp3"),
            managed_path: blob_relative(hash, "mp3"),
            available: true,
        }
    }

    fn temp_root(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "vibedeck-store-{label}-{}",
            super::super::unique_suffix()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn library_names_are_normalised_and_unique() {
        let mut store = CatalogStore::empty();
        store.create_library("  Chill   Mix ", 1).unwrap();
        assert_eq!(store.libraries[0].name, "Chill Mix");
        assert!(store.create_library("chill mix", 2).is_err());
        assert!(store.create_library("   ", 2).is_err());
        assert!(store.create_library(&"x".repeat(81), 2).is_err());
    }

    #[test]
    fn renaming_keeps_the_id_and_rejects_taken_names() {
        let mut store = CatalogStore::empty();
        store.create_library("A", 1).unwrap();
        store.create_library("B", 1).unwrap();
        let id = store.libraries[0].id.clone();
        store.rename_library(&id, "A renamed").unwrap();
        assert_eq!(store.libraries[0].id, id);
        assert_eq!(store.libraries[0].name, "A renamed");
        assert!(store.rename_library(&id, "b").is_err());
        store.rename_library(&id, "a RENAMED").unwrap();
        assert!(store.rename_library("missing", "C").is_err());
    }

    #[test]
    fn a_deleted_name_can_be_reused_with_a_fresh_id_when_the_old_one_is_taken() {
        let mut store = CatalogStore::empty();
        store.create_library("Mix", 1).unwrap();
        let first = store.libraries[0].id.clone();
        store.rename_library(&first, "Other").unwrap();
        store.create_library("Mix", 2).unwrap();
        assert_ne!(store.libraries[1].id, first);
        assert!(store.libraries[1].id.starts_with(&first));
        assert!(is_safe_component(&store.libraries[1].id));
    }

    #[test]
    fn a_track_can_live_in_several_libraries_with_one_file() {
        let mut store = CatalogStore::empty();
        let a = store.create_library("A", 1).unwrap();
        let b = store.create_library("B", 1).unwrap();
        assert_eq!(store.add_track(a, track("h1")), AddOutcome::AddedNew);
        assert_eq!(store.add_track(a, track("h1")), AddOutcome::AlreadyInLibrary);
        assert_eq!(store.add_track(b, track("h1")), AddOutcome::AddedExisting);
        assert_eq!(store.tracks.len(), 1);
        assert_eq!(store.view().playlists[1].tracks.len(), 1);
    }

    #[test]
    fn removing_a_reference_or_a_library_leaves_the_file_as_an_orphan() {
        let mut store = CatalogStore::empty();
        let a = store.create_library("A", 1).unwrap();
        let b = store.create_library("B", 1).unwrap();
        store.add_track(a, track("h1"));
        store.add_track(b, track("h1"));
        store.add_track(b, track("h2"));
        let none = HashSet::new();
        assert!(store.orphans(&none).is_empty());
        let a_id = store.libraries[a].id.clone();
        assert!(store.remove_track_ref(&a_id, "local-h1"));
        assert!(!store.remove_track_ref(&a_id, "local-h1"));
        assert!(store.orphans(&none).is_empty(), "still listed by B");
        let b_id = store.libraries[b].id.clone();
        assert!(store.delete_library(&b_id));
        assert!(!store.delete_library(&b_id));
        let orphans: Vec<_> = store.orphans(&none).iter().map(|t| t.id.clone()).collect();
        assert_eq!(orphans, vec!["local-h1".to_string(), "local-h2".to_string()]);
    }

    #[test]
    fn favourites_protect_files_from_the_orphan_list() {
        let mut store = CatalogStore::empty();
        let a = store.create_library("A", 1).unwrap();
        store.add_track(a, track("h1"));
        store.add_track(a, track("h2"));
        let id = store.libraries[a].id.clone();
        store.delete_library(&id);
        let protected: HashSet<String> = ["local-h2".to_string()].into();
        let orphans: Vec<_> = store.orphans(&protected).iter().map(|t| t.id.clone()).collect();
        assert_eq!(orphans, vec!["local-h1".to_string()]);
        let removed = store.remove_tracks(&["local-h1".to_string()].into());
        assert_eq!(removed.len(), 1);
        assert!(store.track("local-h1").is_none());
        assert!(store.track("local-h2").is_some());
    }

    #[test]
    fn moving_tracks_between_libraries() {
        let mut store = CatalogStore::empty();
        let a = store.create_library("A", 1).unwrap();
        let b = store.create_library("B", 1).unwrap();
        store.add_track(a, track("h1"));
        store.add_track(a, track("h2"));
        store.add_track(b, track("h2"));
        let (a_id, b_id) = (store.libraries[a].id.clone(), store.libraries[b].id.clone());
        let moved = store
            .move_tracks(&a_id, &b_id, &["local-h1".into(), "local-h2".into(), "local-zz".into()])
            .unwrap();
        assert_eq!(moved, 2);
        assert!(store.libraries[a].track_ids.is_empty());
        assert_eq!(store.libraries[b].track_ids, vec!["local-h2".to_string(), "local-h1".to_string()]);
        assert_eq!(store.move_tracks(&a_id, &a_id, &[]).unwrap(), 0);
        assert!(store.move_tracks(&a_id, "nope", &[]).is_err());
    }

    #[test]
    fn find_or_create_reuses_a_library_by_name() {
        let mut store = CatalogStore::empty();
        let first = store.find_or_create_library("Rock", 1).unwrap();
        assert_eq!(store.find_or_create_library(" rock ", 2).unwrap(), first);
        assert_eq!(store.libraries.len(), 1);
    }

    #[test]
    fn migration_moves_files_into_blobs_and_is_repeatable() {
        let root = temp_root("migrate");
        let old_dir = root.join("playlists").join("local-abc");
        fs::create_dir_all(&old_dir).unwrap();
        fs::write(old_dir.join("h1.mp3"), b"one").unwrap();
        let mut v1_track = track("h1");
        v1_track.managed_path = "playlists/local-abc/h1.mp3".to_string();
        let missing = {
            let mut t = track("h2");
            t.managed_path = "playlists/local-abc/h2.mp3".to_string();
            t
        };
        let raw = serde_json::json!({
            "version": 1,
            "playlists": [{ "id": "local-abc", "name": "Old", "tracks": [v1_track, missing] }]
        })
        .to_string();

        let store = migrate_legacy(&root, &raw).unwrap();
        assert_eq!(store.version, CATALOG_VERSION);
        assert_eq!(store.libraries[0].track_ids.len(), 2);
        assert_eq!(store.track("local-h1").unwrap().managed_path, "blobs/h1.mp3");
        assert_eq!(fs::read(root.join("blobs/h1.mp3")).unwrap(), b"one");
        assert!(!old_dir.join("h1.mp3").exists());
        assert!(!root.join("blobs/h2.mp3").exists(), "a missing file stays missing");

        let again = migrate_legacy(&root, &raw).unwrap();
        assert_eq!(again, store);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn migration_rejects_unsafe_references() {
        let root = temp_root("unsafe");
        let mut bad = track("h1");
        bad.content_hash = "../escape".to_string();
        let raw = serde_json::json!({
            "version": 1,
            "playlists": [{ "id": "local-abc", "name": "Old", "tracks": [bad] }]
        })
        .to_string();
        assert!(migrate_legacy(&root, &raw).is_err());
        assert!(migrate_legacy(&root, "{\"version\":3,\"playlists\":[]}").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

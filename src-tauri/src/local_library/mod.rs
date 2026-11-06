use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tauri::{AppHandle, Emitter, Manager};
mod store;

use store::{blob_relative, AddOutcome, CatalogStore, CATALOG_VERSION};

const SUPPORTED_EXTENSIONS: [&str; 8] = ["mp3", "flac", "m4a", "aac", "wav", "ogg", "opus", "webm"];

static CATALOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CANCELLED_OPERATIONS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalLibraryCatalog {
    pub version: u32,
    pub playlists: Vec<LocalPlaylist>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlaylist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<LocalTrack>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalTrack {
    pub id: String,
    pub content_hash: String,
    pub byte_length: u64,
    pub extension: String,
    pub file_name: String,
    pub managed_path: String,
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLocalPathsRequest {
    pub paths: Vec<String>,
    /// Library that receives loose files when `library_id` is absent (created on demand).
    #[serde(default)]
    pub playlist_name: String,
    #[serde(default)]
    pub library_id: Option<String>,
    /// A dropped or picked folder becomes a library named after the folder.
    #[serde(default)]
    pub folder_libraries: bool,
    /// With `folder_libraries`, each subfolder becomes its own library instead of being flattened.
    #[serde(default)]
    pub subfolders_as_libraries: bool,
    pub operation_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveLocalTrackRequest {
    pub playlist_id: String,
    pub track_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveLocalTrackRequest {
    pub track_id: Option<String>,
    pub managed_path: Option<String>,
    pub correlation_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlaybackBridgeError {
    pub code: String,
    pub operation: String,
    pub message: String,
    pub correlation_id: String,
    pub retryable: bool,
    pub details: Option<serde_json::Value>,
}

impl LocalPlaybackBridgeError {
    pub fn from_error(error: LocalPlaybackError, correlation_id: String) -> Self {
        let code = serde_json::to_value(&error)
            .ok()
            .and_then(|value| {
                value
                    .get("code")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "local_playback_error".to_string());
        let retryable = matches!(
            &error,
            LocalPlaybackError::Io(_) | LocalPlaybackError::NoOutputDevice(_)
        );
        let details = match &error {
            LocalPlaybackError::InvalidContainer(detail)
            | LocalPlaybackError::DecoderFailure(detail)
            | LocalPlaybackError::NoOutputDevice(detail) => Some(serde_json::json!({
                "detail": detail.chars().take(240).collect::<String>()
            })),
            _ => None,
        };
        let message = match code.as_str() {
            "missingFile" => "O arquivo de áudio local não está disponível.".to_string(),
            "unsupportedContainer" => "O formato do arquivo local não é compatível.".to_string(),
            "invalidContainer" => "O contêiner do arquivo local é inválido.".to_string(),
            "decoderFailure" => "O decodificador não conseguiu abrir o áudio local.".to_string(),
            "noOutputDevice" => "Nenhum dispositivo de saída de áudio está disponível.".to_string(),
            _ => error.to_string(),
        };
        Self {
            code,
            operation: "play_local_audio".to_string(),
            message,
            correlation_id,
            retryable,
            details,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "code", content = "detail")]
pub enum LocalPlaybackError {
    MissingTrackReference,
    TrackNotFound,
    UnsafePath,
    OutsideManagedRoot,
    MissingFile,
    UnsupportedContainer,
    InvalidContainer(String),
    DecoderFailure(String),
    NoOutputDevice(String),
    Catalog(String),
    Io(String),
}

impl std::fmt::Display for LocalPlaybackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTrackReference => {
                formatter.write_str("A local track id or managed path is required.")
            }
            Self::TrackNotFound => {
                formatter.write_str("The local track is not present in the catalog.")
            }
            Self::UnsafePath => formatter.write_str("The managed path is unsafe."),
            Self::OutsideManagedRoot => {
                formatter.write_str("The managed path is outside the library root.")
            }
            Self::MissingFile => formatter.write_str("The managed local audio file is missing."),
            Self::UnsupportedContainer => {
                formatter.write_str("The local audio container is not supported.")
            }
            Self::InvalidContainer(detail) => {
                write!(formatter, "The local audio container is invalid: {detail}")
            }
            Self::DecoderFailure(detail) => {
                write!(formatter, "The local audio decoder failed: {detail}")
            }
            Self::NoOutputDevice(detail) => {
                write!(formatter, "No audio output device is available: {detail}")
            }
            Self::Catalog(error) | Self::Io(error) => formatter.write_str(error),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadLocalLibraryResult {
    pub catalog: LocalLibraryCatalog,
    pub stale_track_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLocalPathsResult {
    pub operation_id: String,
    pub files: Vec<LocalImportFileResult>,
    pub cancelled: bool,
    pub catalog: LocalLibraryCatalog,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalImportFileResult {
    pub path: String,
    pub status: LocalImportStatus,
    pub track_id: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalImportStatus {
    Imported,
    Skipped,
    Unsupported,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalImportProgress {
    operation_id: String,
    processed: usize,
    total: usize,
    current_path: String,
    terminal: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelLocalImportResult {
    pub operation_id: String,
    pub accepted: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveLocalTrackResult {
    pub catalog: LocalLibraryCatalog,
    pub removed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameLocalLibraryRequest {
    pub library_id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveLocalTracksRequest {
    pub from_library_id: String,
    pub to_library_id: String,
    pub track_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryMutationResult {
    pub catalog: LocalLibraryCatalog,
    pub library_id: Option<String>,
    pub changed: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanReport {
    pub count: usize,
    pub bytes: u64,
    pub track_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeOrphansResult {
    pub removed_count: usize,
    pub freed_bytes: u64,
    pub catalog: LocalLibraryCatalog,
}

#[tauri::command]
pub fn load_local_library(app: AppHandle) -> Result<LoadLocalLibraryResult, String> {
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    let store = load_catalog(&root)?;
    let catalog = view_with_availability(&root, &store);
    let stale_track_count = store
        .tracks
        .iter()
        .filter(|track| !blob_is_file(&root, &track.managed_path))
        .count();
    Ok(LoadLocalLibraryResult {
        catalog,
        stale_track_count,
    })
}

pub fn resolve_local_track(
    app: &AppHandle,
    request: ResolveLocalTrackRequest,
) -> Result<PathBuf, LocalPlaybackError> {
    if request
        .track_id
        .as_deref()
        .unwrap_or_default()
        .trim()
        .is_empty()
        && request
            .managed_path
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err(LocalPlaybackError::MissingTrackReference);
    }
    let root = library_root(app).map_err(LocalPlaybackError::Io)?;
    let catalog = load_catalog(&root).map_err(LocalPlaybackError::Catalog)?;
    let track = catalog
        .tracks
        .iter()
        .find(|track| {
            request.track_id.as_deref() == Some(track.id.as_str())
                || (request.track_id.is_none()
                    && request.managed_path.as_deref() == Some(track.managed_path.as_str()))
        })
        .ok_or(LocalPlaybackError::TrackNotFound)?;
    if !SUPPORTED_EXTENSIONS.contains(&track.extension.as_str()) {
        return Err(LocalPlaybackError::UnsupportedContainer);
    }
    let relative = request
        .managed_path
        .as_deref()
        .unwrap_or(&track.managed_path);
    if relative != track.managed_path {
        return Err(LocalPlaybackError::TrackNotFound);
    }
    let candidate = root.join(relative);
    if !is_safe_relative(&candidate, &root) {
        return Err(LocalPlaybackError::UnsafePath);
    }
    let canonical_root = fs::canonicalize(&root).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            LocalPlaybackError::MissingFile
        } else {
            LocalPlaybackError::Io(error.to_string())
        }
    })?;
    let canonical = fs::canonicalize(&candidate).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            LocalPlaybackError::MissingFile
        } else {
            LocalPlaybackError::Io(error.to_string())
        }
    })?;
    if !canonical.starts_with(&canonical_root) {
        return Err(LocalPlaybackError::OutsideManagedRoot);
    }
    if !canonical.is_file() {
        return Err(LocalPlaybackError::MissingFile);
    }
    Ok(canonical)
}

#[tauri::command]
pub fn import_local_paths(
    app: AppHandle,
    request: ImportLocalPathsRequest,
) -> Result<ImportLocalPathsResult, String> {
    if request.operation_id.trim().is_empty() {
        return Err("operationId is required".to_string());
    }
    clear_cancelled(&request.operation_id);
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    fs::create_dir_all(root.join("blobs")).map_err(io_error)?;
    let files = enumerate_paths(
        &request.paths,
        request.folder_libraries,
        request.subfolders_as_libraries,
    )?;
    let total = files.len();
    let mut results = Vec::with_capacity(total);
    let mut store = load_catalog(&root)?;
    let now = unix_seconds();
    let mut processed = 0;

    for entry in files {
        let path = entry.path;
        if is_cancelled(&request.operation_id) {
            results.push(import_result(
                path,
                LocalImportStatus::Cancelled,
                None,
                Some("Import cancelled".to_string()),
            ));
            processed += 1;
            emit_progress(&app, &request.operation_id, processed, total, "", false);
            continue;
        }
        let result = if let Some(error) = entry.error {
            import_result(path, LocalImportStatus::Failed, None, Some(error))
        } else {
            match extension(&path) {
                None => import_result(
                    path,
                    LocalImportStatus::Unsupported,
                    None,
                    Some("Unsupported extension".to_string()),
                ),
                Some(extension) => {
                    let target = import_target(&mut store, &request, entry.group.as_deref(), now);
                    match target
                        .and_then(|index| import_file(&root, &mut store, index, &path, extension))
                    {
                        Ok(value) => value,
                        Err(error) => {
                            import_result(path, LocalImportStatus::Failed, None, Some(error))
                        }
                    }
                }
            }
        };
        let current_path = result.path.clone();
        results.push(result);
        processed += 1;
        emit_progress(
            &app,
            &request.operation_id,
            processed,
            total,
            &current_path,
            false,
        );
    }
    let cancelled = is_cancelled(&request.operation_id);
    if results
        .iter()
        .any(|item| matches!(&item.status, LocalImportStatus::Imported))
    {
        save_catalog(&root, &store)?;
    }
    clear_cancelled(&request.operation_id);
    emit_progress(&app, &request.operation_id, processed, total, "", true);
    Ok(ImportLocalPathsResult {
        operation_id: request.operation_id,
        files: results,
        cancelled,
        catalog: view_with_availability(&root, &store),
    })
}

/// Picks the library for one file: its folder's library, the explicit target, or the named default.
fn import_target(
    store: &mut CatalogStore,
    request: &ImportLocalPathsRequest,
    group: Option<&str>,
    now: u64,
) -> Result<usize, String> {
    if let Some(name) = group {
        return store.find_or_create_library(name, now);
    }
    if let Some(id) = request.library_id.as_deref() {
        return store
            .library_index(id)
            .ok_or_else(|| "Biblioteca de destino não encontrada.".to_string());
    }
    store.find_or_create_library(&request.playlist_name, now)
}

#[tauri::command]
pub fn cancel_local_import(operation_id: String) -> CancelLocalImportResult {
    if operation_id.trim().is_empty() {
        return CancelLocalImportResult {
            operation_id,
            accepted: false,
        };
    }
    CANCELLED_OPERATIONS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map(|mut state| {
            state.insert(operation_id.clone());
        })
        .unwrap_or_default();
    CancelLocalImportResult {
        operation_id,
        accepted: true,
    }
}

/// Takes a track out of one library. The file stays until the orphan purge removes it.
#[tauri::command]
pub fn remove_local_track(
    app: AppHandle,
    request: RemoveLocalTrackRequest,
) -> Result<RemoveLocalTrackResult, String> {
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    let mut store = load_catalog(&root)?;
    let removed = store.remove_track_ref(&request.playlist_id, &request.track_id);
    if removed {
        save_catalog(&root, &store)?;
    }
    Ok(RemoveLocalTrackResult {
        catalog: view_with_availability(&root, &store),
        removed,
    })
}

#[tauri::command]
pub fn create_local_library(app: AppHandle, name: String) -> Result<LibraryMutationResult, String> {
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    let mut store = load_catalog(&root)?;
    let index = store.create_library(&name, unix_seconds())?;
    let library_id = store.libraries[index].id.clone();
    save_catalog(&root, &store)?;
    Ok(LibraryMutationResult {
        catalog: view_with_availability(&root, &store),
        library_id: Some(library_id),
        changed: 1,
    })
}

#[tauri::command]
pub fn rename_local_library(
    app: AppHandle,
    request: RenameLocalLibraryRequest,
) -> Result<LibraryMutationResult, String> {
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    let mut store = load_catalog(&root)?;
    store.rename_library(&request.library_id, &request.name)?;
    save_catalog(&root, &store)?;
    Ok(LibraryMutationResult {
        catalog: view_with_availability(&root, &store),
        library_id: Some(request.library_id),
        changed: 1,
    })
}

/// Removes the library and its references. Files are kept for the orphan purge.
#[tauri::command]
pub fn delete_local_library(
    app: AppHandle,
    library_id: String,
) -> Result<LibraryMutationResult, String> {
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    let mut store = load_catalog(&root)?;
    let removed = store.delete_library(&library_id);
    if removed {
        save_catalog(&root, &store)?;
    }
    Ok(LibraryMutationResult {
        catalog: view_with_availability(&root, &store),
        library_id: None,
        changed: usize::from(removed),
    })
}

#[tauri::command]
pub fn move_local_tracks(
    app: AppHandle,
    request: MoveLocalTracksRequest,
) -> Result<LibraryMutationResult, String> {
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    let mut store = load_catalog(&root)?;
    let moved = store.move_tracks(
        &request.from_library_id,
        &request.to_library_id,
        &request.track_ids,
    )?;
    if moved > 0 {
        save_catalog(&root, &store)?;
    }
    Ok(LibraryMutationResult {
        catalog: view_with_availability(&root, &store),
        library_id: Some(request.to_library_id),
        changed: moved,
    })
}

/// Files no library lists and no favourite keeps alive, with the space they occupy.
#[tauri::command]
pub fn preview_orphan_tracks(app: AppHandle) -> Result<OrphanReport, String> {
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    let store = load_catalog(&root)?;
    orphan_report(&root, &store)
}

#[tauri::command]
pub fn purge_orphan_tracks(app: AppHandle) -> Result<PurgeOrphansResult, String> {
    let _guard = lock_catalog()?;
    let root = library_root(&app)?;
    let mut store = load_catalog(&root)?;
    let (removed_count, freed_bytes) = purge_orphans(&root, &mut store)?;
    if removed_count > 0 {
        save_catalog(&root, &store)?;
    }
    Ok(PurgeOrphansResult {
        removed_count,
        freed_bytes,
        catalog: view_with_availability(&root, &store),
    })
}

fn orphan_report(root: &Path, store: &CatalogStore) -> Result<OrphanReport, String> {
    let protected = crate::favorites::favorite_ids(root, "local")?;
    let orphans = store.orphans(&protected);
    Ok(OrphanReport {
        count: orphans.len(),
        bytes: orphans
            .iter()
            .filter(|track| blob_is_file(root, &track.managed_path))
            .map(|track| track.byte_length)
            .sum(),
        track_ids: orphans.iter().map(|track| track.id.clone()).collect(),
    })
}

/// Deletes the orphan files first, then drops their entries, so a crash leaves at worst
/// entries whose file is gone (shown as unavailable), never a file nobody can find.
fn purge_orphans(root: &Path, store: &mut CatalogStore) -> Result<(usize, u64), String> {
    let protected = crate::favorites::favorite_ids(root, "local")?;
    let orphans: Vec<LocalTrack> = store.orphans(&protected).into_iter().cloned().collect();
    let mut freed = 0;
    for track in &orphans {
        let path = managed_blob_path(root, &track.managed_path)?;
        match fs::remove_file(&path) {
            Ok(()) => freed += track.byte_length,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    let ids: HashSet<String> = orphans.iter().map(|track| track.id.clone()).collect();
    store.remove_tracks(&ids);
    Ok((orphans.len(), freed))
}

/// Copies the file into `blobs/` (unless it is already there) and lists it in the library.
fn import_file(
    root: &Path,
    store: &mut CatalogStore,
    library: usize,
    source: &Path,
    extension: String,
) -> Result<LocalImportFileResult, String> {
    let (content_hash, byte_length) = hash_file(source)?;
    let track_id = format!("local-{content_hash}");
    if store.libraries[library].track_ids.contains(&track_id) {
        return Ok(import_result(
            source.to_path_buf(),
            LocalImportStatus::Skipped,
            None,
            Some("Duplicate content".to_string()),
        ));
    }
    let relative = blob_relative(&content_hash, &extension);
    let destination = managed_blob_path(root, &relative)?;
    if !destination.exists() {
        fs::create_dir_all(
            destination
                .parent()
                .ok_or_else(|| "Invalid managed path".to_string())?,
        )
        .map_err(io_error)?;
        let temp = destination.with_extension(format!("{}.tmp", unique_suffix()));
        copy_file(source, &temp)?;
        if destination.exists() {
            fs::remove_file(&temp).map_err(io_error)?;
        } else {
            fs::rename(&temp, &destination).map_err(io_error)?;
        }
    }
    let filename = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("track")
        .to_string();
    let track = LocalTrack {
        id: track_id.clone(),
        content_hash: content_hash.clone(),
        byte_length,
        extension: extension.clone(),
        file_name: sanitize_filename(&filename, &content_hash, &extension),
        managed_path: relative,
        available: true,
    };
    let message = match store.add_track(library, track) {
        AddOutcome::AddedNew => None,
        AddOutcome::AddedExisting => {
            Some("Já existia em outra biblioteca; adicionada por referência.".to_string())
        }
        AddOutcome::AlreadyInLibrary => {
            return Ok(import_result(
                source.to_path_buf(),
                LocalImportStatus::Skipped,
                None,
                Some("Duplicate content".to_string()),
            ))
        }
    };
    Ok(import_result(
        source.to_path_buf(),
        LocalImportStatus::Imported,
        Some(track_id),
        message,
    ))
}

fn load_catalog(root: &Path) -> Result<CatalogStore, String> {
    let path = root.join("catalog.json");
    let raw = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(CatalogStore::empty()),
        Err(error) => return Err(io_error(error)),
    };
    let version = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|value| value.get("version").and_then(serde_json::Value::as_u64));
    match version {
        Some(value) if value == u64::from(CATALOG_VERSION) => {
            match serde_json::from_str::<CatalogStore>(&raw) {
                Ok(store) => Ok(store),
                Err(error) => preserve_corrupt_catalog(&path, &error.to_string()),
            }
        }
        // The v1 catalog stays on disk as a `.previous-*` backup after this first save.
        Some(1) => match store::migrate_legacy(root, &raw) {
            Ok(store) => {
                save_catalog(root, &store)?;
                Ok(store)
            }
            Err(error) => Err(format!("Catalog migration failed: {error}")),
        },
        _ => preserve_corrupt_catalog(&path, "unsupported catalog version"),
    }
}

fn save_catalog(root: &Path, catalog: &CatalogStore) -> Result<(), String> {
    let mut value = catalog.clone();
    value.version = CATALOG_VERSION;
    fs::create_dir_all(root).map_err(io_error)?;
    let path = root.join("catalog.json");
    let temp = root.join(format!("catalog.json.{}.tmp", unique_suffix()));
    let bytes = serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?;
    fs::write(&temp, bytes).map_err(io_error)?;
    if path.exists() {
        let backup = root.join(format!("catalog.json.previous-{}", unique_suffix()));
        fs::rename(&path, backup).map_err(io_error)?;
    }
    fs::rename(temp, path).map_err(io_error)
}

fn lock_catalog() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    CATALOG_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Catalog lock poisoned".to_string())
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

fn blob_is_file(root: &Path, relative: &str) -> bool {
    managed_blob_path(root, relative).map(|path| path.is_file()).unwrap_or(false)
}

/// The interface view of the catalog with each track's `available` flag checked on disk.
fn view_with_availability(root: &Path, store: &CatalogStore) -> LocalLibraryCatalog {
    let mut checked = store.clone();
    for track in &mut checked.tracks {
        track.available = blob_is_file(root, &track.managed_path);
    }
    checked.view()
}

pub(crate) fn library_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("library"))
}

/// A managed file is always `blobs/<name>` directly under the library root.
fn managed_blob_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = root.join(relative);
    if !is_safe_relative(&path, root) {
        return Err("Unsafe managed path".to_string());
    }
    let name = relative
        .strip_prefix("blobs/")
        .ok_or_else(|| "Managed path is outside library root".to_string())?;
    if !is_safe_component(name) {
        return Err("Unsafe managed path".to_string());
    }
    Ok(path)
}

fn is_safe_relative(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .ok()
        .map(|relative| {
            relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        })
        .unwrap_or(false)
}
fn is_safe_component(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\'])
}
#[cfg(test)]
fn stable_playlist_id(name: &str) -> String {
    store::stable_library_id(name)
}
fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(str::to_lowercase)
        .filter(|value| SUPPORTED_EXTENSIONS.contains(&value.as_str()))
}
fn sanitize_filename(name: &str, hash: &str, extension: &str) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("track");
    let clean = stem
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .collect::<String>();
    format!(
        "{}.{}",
        if clean.is_empty() {
            format!("track-{hash}")
        } else {
            clean
        },
        extension
    )
}
fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(io_error)?;
    let length = file.metadata().map_err(io_error)?.len();
    let mut hash = Sha1::new();
    hash.update(length.to_le_bytes());
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok((hex(&hash.finalize()), length))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    let mut input = File::open(source).map_err(io_error)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(io_error)?;
    io::copy(&mut input, &mut output).map_err(io_error)?;
    output.flush().map_err(io_error)
}
struct ImportEntry {
    path: PathBuf,
    error: Option<String>,
    /// Library named after the folder this file came from, when folders become libraries.
    group: Option<String>,
}

fn enumerate_paths(
    paths: &[String],
    folder_libraries: bool,
    subfolders_as_libraries: bool,
) -> Result<Vec<ImportEntry>, String> {
    let mut files = Vec::new();
    for value in paths {
        let path = Path::new(value);
        let group = if folder_libraries && path.is_dir() {
            folder_name(path)
        } else {
            None
        };
        enumerate_path(path, &mut files, group, subfolders_as_libraries)?;
    }
    Ok(files)
}

fn folder_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn enumerate_path(
    path: &Path,
    files: &mut Vec<ImportEntry>,
    group: Option<String>,
    subfolders_as_libraries: bool,
) -> Result<(), String> {
    let failed = |error: io::Error| ImportEntry {
        path: path.to_path_buf(),
        error: Some(io_error(error)),
        group: None,
    };
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            files.push(failed(error));
            return Ok(());
        }
    };
    if metadata.is_dir() {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                files.push(failed(error));
                return Ok(());
            }
        };
        let mut children: Vec<PathBuf> = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => children.push(entry.path()),
                Err(error) => files.push(failed(error)),
            }
        }
        children.sort();
        for child in children {
            let child_group = if subfolders_as_libraries && group.is_some() && child.is_dir() {
                folder_name(&child)
            } else {
                group.clone()
            };
            enumerate_path(&child, files, child_group, subfolders_as_libraries)?;
        }
    } else {
        files.push(ImportEntry {
            path: path.to_path_buf(),
            error: None,
            group,
        });
    }
    Ok(())
}
fn preserve_corrupt_catalog(path: &Path, reason: &str) -> Result<CatalogStore, String> {
    let backup = path.with_file_name(format!("catalog.json.corrupt-{}", unique_suffix()));
    let _ = fs::rename(path, backup);
    Err(format!("Catalog is corrupt and was preserved: {reason}"))
}
fn import_result(
    path: PathBuf,
    status: LocalImportStatus,
    track_id: Option<String>,
    message: Option<String>,
) -> LocalImportFileResult {
    LocalImportFileResult {
        path: path.to_string_lossy().to_string(),
        status,
        track_id,
        message,
    }
}
fn emit_progress(
    app: &AppHandle,
    operation_id: &str,
    processed: usize,
    total: usize,
    current_path: &str,
    terminal: bool,
) {
    let _ = app.emit(
        "local-import-progress",
        LocalImportProgress {
            operation_id: operation_id.to_string(),
            processed,
            total,
            current_path: current_path.to_string(),
            terminal,
        },
    );
}
fn is_cancelled(operation_id: &str) -> bool {
    CANCELLED_OPERATIONS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map(|state| state.contains(operation_id))
        .unwrap_or(false)
}
fn clear_cancelled(operation_id: &str) {
    if let Ok(mut state) = CANCELLED_OPERATIONS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        state.remove(operation_id);
    }
}
fn unique_suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
fn io_error(error: io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn filters_supported_extensions_case_insensitively() {
        assert_eq!(extension(Path::new("song.MP3")), Some("mp3".to_string()));
        assert_eq!(extension(Path::new("song.exe")), None);
    }

    #[test]
    fn playlist_ids_are_stable_and_path_safe() {
        assert_eq!(
            stable_playlist_id("  Chill   Mix "),
            stable_playlist_id("chill mix")
        );
        assert!(is_safe_component(&stable_playlist_id("../escape")));
        assert!(!is_safe_relative(
            Path::new("/tmp/library/../outside"),
            Path::new("/tmp/library")
        ));
        assert!(!is_safe_relative(
            Path::new("/tmp/library/playlists/../outside.mp3"),
            Path::new("/tmp/library")
        ));
        assert!(!is_safe_relative(
            Path::new("/tmp/other/file.mp3"),
            Path::new("/tmp/library")
        ));
    }

    #[test]
    fn local_playback_errors_remain_typed_for_missing_files() {
        let error = LocalPlaybackError::MissingFile;
        let value = serde_json::to_value(error).unwrap();
        assert_eq!(value["code"], "missingFile");
    }

    #[test]
    fn local_playback_errors_expose_actionable_failure_categories() {
        assert_eq!(
            serde_json::to_value(LocalPlaybackError::DecoderFailure("bad codec".to_string()))
                .unwrap()["code"],
            "decoderFailure"
        );
        assert_eq!(
            serde_json::to_value(LocalPlaybackError::NoOutputDevice(
                "unavailable".to_string()
            ))
            .unwrap()["code"],
            "noOutputDevice"
        );
        for error in [
            LocalPlaybackError::MissingFile,
            LocalPlaybackError::InvalidContainer("invalid".to_string()),
            LocalPlaybackError::DecoderFailure("decoder".to_string()),
            LocalPlaybackError::NoOutputDevice("device".to_string()),
        ] {
            let bridge = LocalPlaybackBridgeError::from_error(error, "corr-test".to_string());
            assert_eq!(bridge.operation, "play_local_audio");
            assert_eq!(bridge.correlation_id, "corr-test");
            assert!(!bridge.message.contains("[object Object]"));
        }
    }

    #[test]
    fn duplicate_hash_includes_content_length() {
        let directory =
            std::env::temp_dir().join(format!("vibedeck-library-test-{}", unique_suffix()));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("a.mp3"), b"same").unwrap();
        fs::write(directory.join("b.mp3"), b"same").unwrap();
        assert_eq!(
            hash_file(&directory.join("a.mp3")).unwrap(),
            hash_file(&directory.join("b.mp3")).unwrap()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn copy_and_catalog_updates_leave_complete_files() {
        let directory =
            std::env::temp_dir().join(format!("vibedeck-library-test-{}", unique_suffix()));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source.mp3");
        let destination = directory.join("managed.mp3");
        fs::write(&source, b"audio bytes").unwrap();
        copy_file(&source, &destination).unwrap();
        assert_eq!(fs::read(&source).unwrap(), fs::read(&destination).unwrap());
        let catalog = CatalogStore::empty();
        save_catalog(&directory, &catalog).unwrap();
        assert_eq!(load_catalog(&directory).unwrap(), catalog);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn imported_track_id_and_managed_path_are_catalog_playback_references() {
        let directory =
            std::env::temp_dir().join(format!("vibedeck-library-test-{}", unique_suffix()));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("fixture.mp3");
        fs::write(&source, b"safe local fixture bytes").unwrap();
        let root = directory.join("library");
        let mut store = CatalogStore::empty();
        let library = store.create_library("Biblioteca local", 1).unwrap();
        let result = import_file(&root, &mut store, library, &source, "mp3".to_string()).unwrap();
        let track = store.tracks.first().unwrap();
        assert!(matches!(result.status, LocalImportStatus::Imported));
        assert_eq!(result.track_id.as_deref(), Some(track.id.as_str()));
        assert_eq!(track.id, format!("local-{}", track.content_hash));
        assert_eq!(track.managed_path, format!("blobs/{}.mp3", track.content_hash));
        assert_eq!(fs::read(&source).unwrap(), b"safe local fixture bytes");
        assert!(managed_blob_path(&root, &track.managed_path).unwrap().is_file());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn importing_the_same_file_into_another_library_adds_a_reference_not_a_copy() {
        let directory =
            std::env::temp_dir().join(format!("vibedeck-library-test-{}", unique_suffix()));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("song.mp3");
        fs::write(&source, b"same bytes").unwrap();
        let root = directory.join("library");
        let mut store = CatalogStore::empty();
        let a = store.create_library("A", 1).unwrap();
        let b = store.create_library("B", 1).unwrap();
        let first = import_file(&root, &mut store, a, &source, "mp3".to_string()).unwrap();
        let again = import_file(&root, &mut store, a, &source, "mp3".to_string()).unwrap();
        let other = import_file(&root, &mut store, b, &source, "mp3".to_string()).unwrap();
        assert!(matches!(first.status, LocalImportStatus::Imported));
        assert!(matches!(again.status, LocalImportStatus::Skipped));
        assert!(matches!(other.status, LocalImportStatus::Imported));
        assert_eq!(store.tracks.len(), 1);
        assert_eq!(fs::read_dir(root.join("blobs")).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn re_importing_heals_a_track_whose_file_went_missing() {
        let directory =
            std::env::temp_dir().join(format!("vibedeck-library-test-{}", unique_suffix()));
        fs::create_dir_all(&directory).unwrap();
        let source = directory.join("song.mp3");
        fs::write(&source, b"bytes").unwrap();
        let root = directory.join("library");
        let mut store = CatalogStore::empty();
        let a = store.create_library("A", 1).unwrap();
        let b = store.create_library("B", 1).unwrap();
        import_file(&root, &mut store, a, &source, "mp3".to_string()).unwrap();
        let blob = managed_blob_path(&root, &store.tracks[0].managed_path).unwrap();
        fs::remove_file(&blob).unwrap();
        import_file(&root, &mut store, b, &source, "mp3".to_string()).unwrap();
        assert!(blob.is_file());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn purging_orphans_deletes_only_unreferenced_files() {
        let directory =
            std::env::temp_dir().join(format!("vibedeck-library-test-{}", unique_suffix()));
        fs::create_dir_all(&directory).unwrap();
        let root = directory.join("library");
        let mut store = CatalogStore::empty();
        let keep = store.create_library("Keep", 1).unwrap();
        let drop = store.create_library("Drop", 1).unwrap();
        for (name, library) in [("keep.mp3", keep), ("drop.mp3", drop)] {
            let source = directory.join(name);
            fs::write(&source, name.as_bytes()).unwrap();
            import_file(&root, &mut store, library, &source, "mp3".to_string()).unwrap();
        }
        let drop_id = store.libraries[drop].id.clone();
        store.delete_library(&drop_id);
        let report = orphan_report(&root, &store).unwrap();
        assert_eq!(report.count, 1);
        assert_eq!(report.bytes, "drop.mp3".len() as u64);
        let (removed, freed) = purge_orphans(&root, &mut store).unwrap();
        assert_eq!((removed, freed), (1, "drop.mp3".len() as u64));
        assert_eq!(store.tracks.len(), 1);
        assert_eq!(fs::read_dir(root.join("blobs")).unwrap().count(), 1);
        assert_eq!(orphan_report(&root, &store).unwrap().count, 0);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn managed_blob_paths_reject_anything_but_a_blob_name() {
        let root = Path::new("/tmp/library");
        assert!(managed_blob_path(root, "blobs/abc.mp3").is_ok());
        assert!(managed_blob_path(root, "playlists/x/abc.mp3").is_err());
        assert!(managed_blob_path(root, "blobs/../abc.mp3").is_err());
        assert!(managed_blob_path(root, "blobs/a/b.mp3").is_err());
        assert!(managed_blob_path(root, "../blobs/abc.mp3").is_err());
    }

    #[test]
    fn folders_become_libraries_and_subfolders_follow_the_option() {
        let directory =
            std::env::temp_dir().join(format!("vibedeck-library-test-{}", unique_suffix()));
        let album = directory.join("Album");
        fs::create_dir_all(album.join("Disc 2")).unwrap();
        fs::write(album.join("a.mp3"), b"a").unwrap();
        fs::write(album.join("Disc 2").join("b.mp3"), b"b").unwrap();
        let loose = directory.join("loose.mp3");
        fs::write(&loose, b"c").unwrap();
        let paths = vec![
            album.to_string_lossy().to_string(),
            loose.to_string_lossy().to_string(),
        ];
        let groups = |nested: bool| -> Vec<(String, Option<String>)> {
            enumerate_paths(&paths, true, nested)
                .unwrap()
                .into_iter()
                .map(|entry| {
                    (
                        entry.path.file_name().unwrap().to_string_lossy().to_string(),
                        entry.group,
                    )
                })
                .collect()
        };
        // Children are visited in path order, so the "Disc 2" folder comes before "a.mp3".
        assert_eq!(
            groups(false),
            vec![
                ("b.mp3".into(), Some("Album".into())),
                ("a.mp3".into(), Some("Album".into())),
                ("loose.mp3".into(), None),
            ]
        );
        assert_eq!(
            groups(true),
            vec![
                ("b.mp3".into(), Some("Disc 2".into())),
                ("a.mp3".into(), Some("Album".into())),
                ("loose.mp3".into(), None),
            ]
        );
        assert!(enumerate_paths(&paths, false, true).unwrap().iter().all(|entry| entry.group.is_none()));
        fs::remove_dir_all(directory).unwrap();
    }
}

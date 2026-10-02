use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tauri::{AppHandle, Emitter, Manager};

const CATALOG_VERSION: u32 = 1;
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
    pub playlist_name: String,
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

#[tauri::command]
pub fn load_local_library(app: AppHandle) -> Result<LoadLocalLibraryResult, String> {
    let root = library_root(&app)?;
    let mut catalog = load_catalog(&root)?;
    let mut stale_track_count = 0;
    for playlist in &mut catalog.playlists {
        for track in &mut playlist.tracks {
            track.available = managed_path(&root, &playlist.id, &track.managed_path)?.is_file();
            if !track.available {
                stale_track_count += 1;
            }
        }
    }
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
        .playlists
        .iter()
        .flat_map(|playlist| playlist.tracks.iter())
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
    let _guard = CATALOG_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Catalog lock poisoned".to_string())?;
    let root = library_root(&app)?;
    fs::create_dir_all(root.join("playlists")).map_err(io_error)?;
    let files = enumerate_paths(&request.paths)?;
    let total = files.len();
    let mut results = Vec::with_capacity(total);
    let mut catalog = load_catalog(&root)?;
    let playlist_id = stable_playlist_id(&request.playlist_name);
    let playlist_index = catalog
        .playlists
        .iter()
        .position(|item| item.id == playlist_id);
    let index = playlist_index.unwrap_or_else(|| {
        catalog.playlists.push(LocalPlaylist {
            id: playlist_id.clone(),
            name: request.playlist_name.clone(),
            tracks: Vec::new(),
        });
        catalog.playlists.len() - 1
    });
    let mut processed = 0;

    for (path, enumeration_error) in files {
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
        let result = if let Some(error) = enumeration_error {
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
                    match import_file(&root, &mut catalog.playlists[index], &path, extension) {
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
    if !catalog.playlists[index].tracks.is_empty()
        && results
            .iter()
            .any(|item| matches!(&item.status, LocalImportStatus::Imported))
    {
        save_catalog(&root, &catalog)?;
    }
    clear_cancelled(&request.operation_id);
    emit_progress(&app, &request.operation_id, processed, total, "", true);
    Ok(ImportLocalPathsResult {
        operation_id: request.operation_id,
        files: results,
        cancelled,
        catalog,
    })
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

#[tauri::command]
pub fn remove_local_track(
    app: AppHandle,
    request: RemoveLocalTrackRequest,
) -> Result<RemoveLocalTrackResult, String> {
    let _guard = CATALOG_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Catalog lock poisoned".to_string())?;
    let root = library_root(&app)?;
    let mut catalog = load_catalog(&root)?;
    let Some(playlist) = catalog
        .playlists
        .iter_mut()
        .find(|item| item.id == request.playlist_id)
    else {
        return Ok(RemoveLocalTrackResult {
            catalog,
            removed: false,
        });
    };
    let Some(position) = playlist
        .tracks
        .iter()
        .position(|track| track.id == request.track_id)
    else {
        return Ok(RemoveLocalTrackResult {
            catalog,
            removed: false,
        });
    };
    let track = playlist.tracks.remove(position);
    let path = managed_path(&root, &playlist.id, &track.managed_path)?;
    if path.exists() {
        fs::remove_file(path).map_err(io_error)?;
    }
    save_catalog(&root, &catalog)?;
    Ok(RemoveLocalTrackResult {
        catalog,
        removed: true,
    })
}

fn import_file(
    root: &Path,
    playlist: &mut LocalPlaylist,
    source: &Path,
    extension: String,
) -> Result<LocalImportFileResult, String> {
    let (content_hash, byte_length) = hash_file(source)?;
    if playlist
        .tracks
        .iter()
        .any(|track| track.content_hash == content_hash && track.byte_length == byte_length)
        || root_catalog_has_duplicate(root, &content_hash, byte_length)?
    {
        return Ok(import_result(
            source.to_path_buf(),
            LocalImportStatus::Skipped,
            None,
            Some("Duplicate content".to_string()),
        ));
    }
    let filename = source
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("track")
        .to_string();
    let safe_name = sanitize_filename(&filename, &content_hash, &extension);
    let relative = format!("playlists/{}/{}.{}", playlist.id, content_hash, extension);
    let destination = managed_path(root, &playlist.id, &relative)?;
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
    let track_id = format!("local-{}", content_hash);
    playlist.tracks.push(LocalTrack {
        id: track_id.clone(),
        content_hash,
        byte_length,
        extension,
        file_name: safe_name,
        managed_path: relative,
        available: true,
    });
    Ok(import_result(
        source.to_path_buf(),
        LocalImportStatus::Imported,
        Some(track_id),
        None,
    ))
}

fn root_catalog_has_duplicate(root: &Path, hash: &str, length: u64) -> Result<bool, String> {
    Ok(load_catalog(root)?
        .playlists
        .into_iter()
        .flat_map(|playlist| playlist.tracks)
        .any(|track| track.content_hash == hash && track.byte_length == length))
}

fn load_catalog(root: &Path) -> Result<LocalLibraryCatalog, String> {
    let path = root.join("catalog.json");
    match fs::read_to_string(&path) {
        Ok(value) => match serde_json::from_str::<LocalLibraryCatalog>(&value) {
            Ok(catalog) if catalog.version == CATALOG_VERSION => Ok(catalog),
            Ok(_) => preserve_corrupt_catalog(&path, "unsupported catalog version"),
            Err(error) => preserve_corrupt_catalog(&path, &error.to_string()),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(LocalLibraryCatalog {
            version: CATALOG_VERSION,
            playlists: Vec::new(),
        }),
        Err(error) => Err(io_error(error)),
    }
}

fn save_catalog(root: &Path, catalog: &LocalLibraryCatalog) -> Result<(), String> {
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

fn library_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("library"))
}

fn managed_path(root: &Path, playlist_id: &str, relative: &str) -> Result<PathBuf, String> {
    let path = root.join(relative);
    if !is_safe_relative(&path, root) || !is_safe_component(playlist_id) {
        return Err("Unsafe managed path".to_string());
    }
    if !path.starts_with(root) || !relative.starts_with(&format!("playlists/{playlist_id}/")) {
        return Err("Managed path is outside library root".to_string());
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
fn stable_playlist_id(name: &str) -> String {
    let normalized = name
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut hash = Sha1::new();
    hash.update(normalized.as_bytes());
    format!("local-{}", hex(&hash.finalize())[..16].to_string())
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
fn enumerate_paths(paths: &[String]) -> Result<Vec<(PathBuf, Option<String>)>, String> {
    let mut files = Vec::new();
    for value in paths {
        enumerate_path(Path::new(value), &mut files)?;
    }
    Ok(files)
}
fn enumerate_path(path: &Path, files: &mut Vec<(PathBuf, Option<String>)>) -> Result<(), String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            files.push((path.to_path_buf(), Some(io_error(error))));
            return Ok(());
        }
    };
    if metadata.is_dir() {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                files.push((path.to_path_buf(), Some(io_error(error))));
                return Ok(());
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => enumerate_path(&entry.path(), files)?,
                Err(error) => files.push((path.to_path_buf(), Some(io_error(error)))),
            }
        }
    } else {
        files.push((path.to_path_buf(), None));
    }
    Ok(())
}
fn preserve_corrupt_catalog(path: &Path, reason: &str) -> Result<LocalLibraryCatalog, String> {
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
        let catalog = LocalLibraryCatalog {
            version: CATALOG_VERSION,
            playlists: Vec::new(),
        };
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
        let playlist_id = stable_playlist_id("Biblioteca local");
        let mut playlist = LocalPlaylist {
            id: playlist_id.clone(),
            name: "Biblioteca local".to_string(),
            tracks: Vec::new(),
        };
        let result = import_file(
            &directory.join("library"),
            &mut playlist,
            &source,
            "mp3".to_string(),
        )
        .unwrap();
        let track = playlist.tracks.first().unwrap();
        assert!(matches!(result.status, LocalImportStatus::Imported));
        assert_eq!(result.track_id.as_deref(), Some(track.id.as_str()));
        assert_eq!(track.id, format!("local-{}", track.content_hash));
        assert_eq!(
            track.managed_path,
            format!("playlists/{}/{}.mp3", playlist_id, track.content_hash)
        );
        assert_eq!(fs::read(&source).unwrap(), b"safe local fixture bytes");
        assert!(managed_path(
            &directory.join("library"),
            &playlist_id,
            &track.managed_path
        )
        .unwrap()
        .is_file());
        fs::remove_dir_all(directory).unwrap();
    }
}

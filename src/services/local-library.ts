import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type { LibraryItem, Playlist } from '../models/library';
import type { NativePlayerActivation, PlayerSnapshot } from '../interfaces/player';
import type { ImportLocalPathsRequest, ImportLocalPathsResult, LoadLocalLibraryResult, LocalImportFileResult, LocalImportProgress, LocalLibraryCatalog, LocalLibraryPlaylist, ManagedTrackReference, RemoveLocalTrackResult } from '../interfaces/library';
import { normalizeBridgeError } from '../interfaces/bridge';

function createCorrelationId() {
  return `local-playback-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

export type LocalLibraryTrack = ManagedTrackReference;
export type { ImportLocalPathsRequest, ImportLocalPathsResult, LoadLocalLibraryResult, LocalImportFileResult, LocalImportProgress, LocalLibraryCatalog, LocalLibraryPlaylist } from '../interfaces/library';

function pathValue(value: unknown): string[] {
  if (typeof value === 'string') return value.trim() ? [value] : [];
  if (Array.isArray(value)) return value.flatMap(pathValue);
  if (!value || typeof value !== 'object') return [];
  const record = value as Record<string, unknown>;
  return pathValue(record.paths ?? record.path ?? record.payload);
}

export function extractNativePaths(payload: unknown): string[] {
  return [...new Set(pathValue(payload))];
}

export function requireNativePaths(payload: unknown): string[] {
  const paths = extractNativePaths(payload);
  if (!paths.length) throw new Error('A operação nativa não forneceu um caminho utilizável.');
  return paths;
}

export function selectedDialogPaths(selection: string | string[] | null): string[] {
  return extractNativePaths(selection);
}

export function shouldApplyLocalOperation(activeOperationId: string | null, activeGeneration: number, operationId: string, generation: number) {
  return Boolean(activeOperationId && activeOperationId === operationId && activeGeneration === generation);
}

export function summarizeLocalImport(files: LocalImportFileResult[]) {
  return {
    imported: files.filter((file) => file.status === 'imported').length,
    duplicates: files.filter((file) => file.status === 'skipped').length,
    unsupported: files.filter((file) => file.status === 'unsupported').length,
    failed: files.filter((file) => file.status === 'failed').length,
    cancelled: files.filter((file) => file.status === 'cancelled').length,
  };
}

export function localPlaybackErrorMessage(error: unknown) {
  const normalized = normalizeBridgeError(error, 'play_local_audio');
  const detail = normalized.details?.detail;
  if (typeof detail === 'string' && detail) return detail;
  if (normalized.code !== 'bridge_error') return `Falha de reprodução local (${normalized.code}).`;
  return normalized.message || 'Não foi possível reproduzir o arquivo local.';
}

export function localPlaybackLogEntry(error: unknown) {
  const normalized = normalizeBridgeError(error, 'play_local_audio');
  return {
    message: normalized.message,
    operation: normalized.operation,
    correlationId: normalized.correlationId,
    level: 'error' as const,
    code: normalized.code,
    details: normalized.details,
  };
}

export function isTauriRuntime() {
  return typeof (globalThis as typeof globalThis & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined';
}

export function localTrackToLibraryItem(track: LocalLibraryTrack): LibraryItem {
  return {
    id: track.id,
    title: track.fileName.replace(/\.[^/.]+$/, ''),
    artist: 'Arquivo local',
    album: 'Biblioteca local',
    duration: '--:--',
    source: 'local',
    accent: '#4ecba6',
    fileName: track.fileName,
    managedPath: track.managedPath,
    available: track.available,
  };
}

export function localPlaylistToPlaylist(playlist: LocalLibraryPlaylist): Playlist {
  return {
    id: playlist.id,
    name: playlist.name,
    source: 'local',
    mode: 'local',
    allowLocal: true,
    tracks: playlist.tracks.map(localTrackToLibraryItem),
  };
}

export class LocalLibraryService {
  load() {
    return invoke<LoadLocalLibraryResult>('load_local_library');
  }

  importPaths(request: ImportLocalPathsRequest) {
    return invoke<ImportLocalPathsResult>('import_local_paths', { request });
  }

  cancel(operationId: string) {
    return invoke<{ operationId: string; accepted: boolean }>('cancel_local_import', { operationId });
  }

  pickPaths(directory = false) {
    return open({ directory, multiple: true, recursive: directory });
  }

  removeTrack(playlistId: string, trackId: string) {
    return invoke<RemoveLocalTrackResult>('remove_local_track', { request: { playlistId, trackId } });
  }

  async playTrack(trackId: string, managedPath: string | undefined, activation: NativePlayerActivation) {
    if (activation.track.source !== 'local' || activation.track.id !== trackId) {
      return Promise.reject(new Error('A ativação da faixa local não corresponde ao catálogo.'));
    }
    const request = { trackId, managedPath, correlationId: createCorrelationId() };
    try {
      const snapshot = await invoke<PlayerSnapshot>('play_local_audio', { request, activation });
      if (!snapshot.nativeMode || snapshot.track?.id !== trackId || snapshot.queueGeneration !== activation.queueGeneration || !snapshot.isPlaying) {
        throw normalizeBridgeError({ code: 'activation_not_confirmed', message: 'O áudio local não confirmou o início da saída de áudio.' }, 'play_local_audio', request.correlationId);
      }
      return snapshot;
    } catch (error) {
      throw normalizeBridgeError(error, 'play_local_audio', request.correlationId);
    }
  }

  listenProgress(callback: (progress: LocalImportProgress) => void): Promise<UnlistenFn> {
    if (!isTauriRuntime()) return Promise.resolve(() => undefined);
    return listen<LocalImportProgress>('local-import-progress', (event) => callback(event.payload));
  }
}

export const localLibraryService = new LocalLibraryService();

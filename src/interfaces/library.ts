import type { LibraryItem, Playlist } from '../models/library';
export type { LibraryItem, LibrarySource, Playlist, PlaylistMode } from '../models/library';

export interface ManagedTrackReference {
  id: string;
  managedPath: string;
  contentHash: string;
  byteLength: number;
  extension: string;
  fileName: string;
  available: boolean;
}

export interface LocalLibraryPlaylist {
  id: string;
  name: string;
  tracks: ManagedTrackReference[];
}

export interface LocalLibraryCatalog {
  version: number;
  playlists: LocalLibraryPlaylist[];
}

export interface LoadLocalLibraryResult {
  catalog: LocalLibraryCatalog;
  staleTrackCount: number;
}

export interface PlaylistCatalogEntry extends Playlist {
  generation: number;
  continuation?: string | null;
  terminal: boolean;
  lastError?: string | null;
}

export interface PlaylistCatalogSnapshot {
  version: number;
  playlists: PlaylistCatalogEntry[];
}

export interface SavePlaylistPageRequest {
  playlistId: string;
  name: string;
  source: string;
  mode: string;
  allowLocal: boolean;
  generation: number;
  tracks: LibraryItem[];
  continuation?: string | null;
  hasMore: boolean;
  error?: string | null;
}

export type LocalImportStatus = 'imported' | 'skipped' | 'unsupported' | 'failed' | 'cancelled';

export interface LocalImportFileResult {
  path: string;
  status: LocalImportStatus;
  trackId?: string;
  message?: string;
}

export interface ImportLocalPathsRequest {
  paths: string[];
  playlistName: string;
  operationId: string;
}

export interface ImportLocalPathsResult {
  operationId: string;
  files: LocalImportFileResult[];
  cancelled: boolean;
  catalog: LocalLibraryCatalog;
}

export interface LocalImportProgress {
  operationId: string;
  processed: number;
  total: number;
  currentPath: string;
  terminal: boolean;
}

export interface RemoveLocalTrackResult {
  catalog: LocalLibraryCatalog;
  removed: boolean;
}

export interface LocalPlaybackReference {
  trackId: string;
  managedPath?: string;
}

export type LocalPlaylistView = Playlist;
export type LocalTrackView = LibraryItem;

export function isManagedTrackReference(value: unknown): value is ManagedTrackReference {
  if (!value || typeof value !== 'object') return false;
  const track = value as Partial<ManagedTrackReference>;
  return typeof track.id === 'string'
    && typeof track.managedPath === 'string'
    && typeof track.contentHash === 'string'
    && typeof track.byteLength === 'number'
    && typeof track.extension === 'string'
    && typeof track.fileName === 'string'
    && typeof track.available === 'boolean';
}

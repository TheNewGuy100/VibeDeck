import type { LocalLibraryCatalog } from '../interfaces/library';

export const MAX_LIBRARY_NAME = 80;

export interface LibrarySummary {
  id: string;
  name: string;
  count: number;
  bytes: number;
  unavailable: number;
}

/** Same rule as the native catalog: collapse whitespace, 1..80 characters. */
export function normalizeLibraryName(name: string): string {
  return name.split(/\s+/).filter(Boolean).join(' ');
}

/** Mirrors the native checks so the dialog can explain a problem before calling the backend. */
export function validateLibraryName(name: string, existing: readonly { id: string; name: string }[], exceptId?: string): string | null {
  const normalized = normalizeLibraryName(name);
  if (!normalized) return 'Informe um nome para a biblioteca.';
  if ([...normalized].length > MAX_LIBRARY_NAME) return `Use no máximo ${MAX_LIBRARY_NAME} caracteres.`;
  const wanted = normalized.toLowerCase();
  if (existing.some((library) => library.id !== exceptId && library.name.toLowerCase() === wanted)) return 'Já existe uma biblioteca com este nome.';
  return null;
}

export function summarizeLibraries(catalog: LocalLibraryCatalog): LibrarySummary[] {
  return catalog.playlists.map((playlist) => ({
    id: playlist.id,
    name: playlist.name,
    count: playlist.tracks.length,
    bytes: playlist.tracks.reduce((total, track) => total + track.byteLength, 0),
    unavailable: playlist.tracks.filter((track) => !track.available).length,
  }));
}

/** A file can sit in several libraries; the "all local tracks" view lists it once. */
export function uniqueLocalTracks<T extends { id: string }>(lists: readonly (readonly T[])[]): T[] {
  const seen = new Set<string>();
  const result: T[] = [];
  for (const list of lists) {
    for (const track of list) {
      if (seen.has(track.id)) continue;
      seen.add(track.id);
      result.push(track);
    }
  }
  return result;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const exponent = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const value = bytes / 1024 ** exponent;
  return `${exponent === 0 || value >= 100 ? Math.round(value) : value.toFixed(1)} ${units[exponent]}`;
}

export function pluralize(count: number, one: string, many: string): string {
  return `${count} ${count === 1 ? one : many}`;
}

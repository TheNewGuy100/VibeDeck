import { invoke } from '@tauri-apps/api/core';
import type { LibraryItem, LibrarySource, Playlist, UnavailableReason } from '../models/library';

/** A favourite is a reference (source + id) plus the minimum metadata to list it offline. */
export interface FavoriteItem {
  source: LibrarySource;
  id: string;
  addedAt: number;
  title: string;
  artist: string;
  album: string;
  duration: string;
  coverUrl?: string | null;
  youtubeId?: string | null;
}

export interface FavoritesStore {
  version: number;
  /** Newest first. */
  items: FavoriteItem[];
}

export type FavoritesScope = 'global' | 'local' | 'youtube';

export interface FavoritesContext {
  youtubeSignedIn: boolean;
}

export const FAVORITES_PLAYLIST_IDS: Record<FavoritesScope, string> = {
  global: 'favorites',
  local: 'favorites-local',
  youtube: 'favorites-youtube',
};

const ACCENTS: Record<LibrarySource, string> = { local: '#4ecba6', youtube: '#ff5f55' };

export function isFavoritesPlaylistId(id: string | null | undefined): boolean {
  return Object.values(FAVORITES_PLAYLIST_IDS).includes(id ?? '');
}

export function favoriteKey(item: Pick<LibraryItem, 'source' | 'id'> | Pick<FavoriteItem, 'source' | 'id'>): string {
  return `${item.source}:${item.id}`;
}

export function toFavoriteItem(item: LibraryItem): FavoriteItem {
  return {
    source: item.source,
    id: item.id,
    addedAt: 0,
    title: item.title,
    artist: item.artist,
    album: item.album,
    duration: item.duration,
    coverUrl: item.coverUrl ?? null,
    youtubeId: item.youtubeId ?? null,
  };
}

/**
 * Rebuilds a playable track. Local favourites take the live catalog entry (it knows the managed
 * file); if the file was purged the track stays listed but unavailable.
 */
export function favoriteToLibraryItem(favorite: FavoriteItem, localCatalog: ReadonlyMap<string, LibraryItem>, context: FavoritesContext = { youtubeSignedIn: true }): LibraryItem {
  if (favorite.source === 'local') {
    const live = localCatalog.get(favorite.id);
    if (live) return live;
  }
  // YouTube favourites stay listed after sign-out but cannot play until the account is back.
  const unavailableReason: UnavailableReason | undefined = favorite.source === 'local'
    ? 'file-missing'
    : context.youtubeSignedIn ? undefined : 'signed-out';
  return {
    id: favorite.id,
    title: favorite.title,
    artist: favorite.artist,
    album: favorite.album,
    duration: favorite.duration,
    source: favorite.source,
    accent: ACCENTS[favorite.source] ?? ACCENTS.local,
    coverUrl: favorite.coverUrl ?? undefined,
    youtubeId: favorite.youtubeId ?? undefined,
    ...(favorite.source === 'local' ? { available: false } : {}),
    ...(unavailableReason ? { unavailableReason } : {}),
  };
}

/** Global is every favourite; Local and YouTube are filters over the same list. */
export function favoritePlaylists(items: readonly FavoriteItem[], localCatalog: ReadonlyMap<string, LibraryItem>, context: FavoritesContext = { youtubeSignedIn: true }): Playlist[] {
  const toTracks = (list: readonly FavoriteItem[]) => list.map((item) => favoriteToLibraryItem(item, localCatalog, context));
  return [
    { id: FAVORITES_PLAYLIST_IDS.global, name: 'Favoritos', source: 'mixed', mode: 'mixed', tracks: toTracks(items), allowLocal: false, terminal: true },
    { id: FAVORITES_PLAYLIST_IDS.local, name: 'Favoritos locais', source: 'local', mode: 'local', tracks: toTracks(items.filter((item) => item.source === 'local')), allowLocal: false, terminal: true },
    { id: FAVORITES_PLAYLIST_IDS.youtube, name: 'Favoritos do YouTube Music', source: 'youtube', mode: 'online', tracks: toTracks(items.filter((item) => item.source === 'youtube')), allowLocal: false, terminal: true },
  ];
}

/** Optimistic list update, mirroring the native rule: newest first, one entry per source + id. */
export function applyFavorite(items: readonly FavoriteItem[], item: FavoriteItem, favorite: boolean, now = Math.floor(Date.now() / 1000)): FavoriteItem[] {
  const key = favoriteKey(item);
  const without = items.filter((existing) => favoriteKey(existing) !== key);
  if (!favorite) return without;
  const previous = items.find((existing) => favoriteKey(existing) === key);
  if (previous) return items.map((existing) => (existing === previous ? { ...item, addedAt: previous.addedAt } : existing));
  return [{ ...item, addedAt: now }, ...without];
}

export class FavoritesService {
  load() {
    return invoke<FavoritesStore>('load_favorites');
  }

  set(item: FavoriteItem, favorite: boolean) {
    return invoke<FavoritesStore>('set_favorite', { item, favorite });
  }
}

export const favoritesService = new FavoritesService();

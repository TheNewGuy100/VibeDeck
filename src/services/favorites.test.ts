import { describe, expect, it } from 'vitest';
import type { LibraryItem } from '../models/library';
import { applyFavorite, FAVORITES_PLAYLIST_IDS, favoriteKey, favoritePlaylists, isFavoritesPlaylistId, toFavoriteItem, type FavoriteItem } from './favorites';

const track = (patch: Partial<LibraryItem> = {}): LibraryItem => ({
  id: 'a', title: 'Song', artist: 'Artist', album: 'Album', duration: '3:00', source: 'youtube', accent: '#fff', ...patch,
});
const favorite = (patch: Partial<FavoriteItem> = {}): FavoriteItem => ({ ...toFavoriteItem(track()), addedAt: 1, ...patch });

describe('favourite keys', () => {
  it('separates the same id coming from different sources', () => {
    expect(favoriteKey(track({ source: 'local' }))).not.toBe(favoriteKey(track({ source: 'youtube' })));
  });

  it('recognises the three favourites playlist ids', () => {
    for (const id of Object.values(FAVORITES_PLAYLIST_IDS)) expect(isFavoritesPlaylistId(id)).toBe(true);
    expect(isFavoritesPlaylistId('local')).toBe(false);
    expect(isFavoritesPlaylistId(null)).toBe(false);
  });
});

describe('applyFavorite', () => {
  it('adds newest first, keeps one entry per source + id and keeps the original date', () => {
    let items: FavoriteItem[] = [];
    items = applyFavorite(items, favorite({ id: 'a' }), true, 10);
    items = applyFavorite(items, favorite({ id: 'b' }), true, 20);
    items = applyFavorite(items, favorite({ id: 'a', title: 'Renamed' }), true, 30);
    expect(items.map((item) => item.id)).toEqual(['b', 'a']);
    expect(items[1]).toMatchObject({ title: 'Renamed', addedAt: 10 });
  });

  it('removes only the matching source', () => {
    const items = [favorite({ source: 'local', id: 'x' }), favorite({ source: 'youtube', id: 'x' })];
    const next = applyFavorite(items, favorite({ source: 'local', id: 'x' }), false);
    expect(next.map(favoriteKey)).toEqual(['youtube:x']);
    expect(applyFavorite(next, favorite({ source: 'local', id: 'x' }), false)).toEqual(next);
  });
});

describe('favoritePlaylists', () => {
  const live = track({ id: 'local-h1', source: 'local', managedPath: 'blobs/h1.mp3', available: true });
  const catalog = new Map([[live.id, live]]);
  const items = [
    favorite({ source: 'youtube', id: 'y1', title: 'Online' }),
    favorite({ source: 'local', id: 'local-h1' }),
    favorite({ source: 'local', id: 'local-gone', title: 'Purged' }),
  ];

  it('builds Global as everything and Local / YouTube as filters of the same list', () => {
    const [global, local, youtube] = favoritePlaylists(items, catalog);
    expect(global.id).toBe('favorites');
    expect(global.tracks).toHaveLength(3);
    expect(local.tracks.map((item) => item.id)).toEqual(['local-h1', 'local-gone']);
    expect(youtube.tracks.map((item) => item.id)).toEqual(['y1']);
  });

  it('plays local favourites through the live catalog entry and marks purged ones unavailable', () => {
    const [, local] = favoritePlaylists(items, catalog);
    expect(local.tracks[0]).toBe(live);
    expect(local.tracks[1]).toMatchObject({ title: 'Purged', available: false });
  });

  it('keeps YouTube favourites listed but unavailable while signed out', () => {
    const [global, , youtube] = favoritePlaylists(items, catalog, { youtubeSignedIn: false });
    expect(youtube.tracks[0]).toMatchObject({ id: 'y1', unavailableReason: 'signed-out' });
    expect(global.tracks.map((item) => [item.id, item.unavailableReason])).toEqual([['y1', 'signed-out'], ['local-h1', undefined], ['local-gone', 'file-missing']]);
    const [, , signedIn] = favoritePlaylists(items, catalog, { youtubeSignedIn: true });
    expect(signedIn.tracks[0].unavailableReason).toBeUndefined();
  });

  it('keeps the YouTube id needed to play online favourites', () => {
    const [, , youtube] = favoritePlaylists([favorite({ id: 'y1', youtubeId: 'vid' })], catalog);
    expect(youtube.tracks[0]).toMatchObject({ youtubeId: 'vid', source: 'youtube' });
  });
});

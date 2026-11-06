import { describe, expect, it } from 'vitest';
import type { LocalLibraryCatalog, ManagedTrackReference } from '../interfaces/library';
import { formatBytes, normalizeLibraryName, pluralize, summarizeLibraries, uniqueLocalTracks, validateLibraryName } from './library-manager';

const track = (id: string, byteLength: number, available = true): ManagedTrackReference => ({
  id, managedPath: `blobs/${id}.mp3`, contentHash: id, byteLength, extension: 'mp3', fileName: `${id}.mp3`, available,
});

describe('validateLibraryName', () => {
  const existing = [{ id: 'a', name: 'Chill Mix' }, { id: 'b', name: 'Rock' }];

  it('accepts a new name and normalises spaces', () => {
    expect(normalizeLibraryName('  Chill   Mix ')).toBe('Chill Mix');
    expect(validateLibraryName('  Jazz  ', existing)).toBeNull();
  });

  it('rejects empty, long and duplicate names, ignoring case', () => {
    expect(validateLibraryName('   ', existing)).toContain('nome');
    expect(validateLibraryName('x'.repeat(81), existing)).toContain('80');
    expect(validateLibraryName('chill  MIX', existing)).toContain('Já existe');
  });

  it('lets a library keep its own name when renaming', () => {
    expect(validateLibraryName('chill mix', existing, 'a')).toBeNull();
    expect(validateLibraryName('rock', existing, 'a')).toContain('Já existe');
  });
});

describe('library summaries', () => {
  const catalog: LocalLibraryCatalog = {
    version: 2,
    playlists: [
      { id: 'l1', name: 'One', tracks: [track('t1', 1000), track('t2', 500, false)] },
      { id: 'l2', name: 'Two', tracks: [track('t1', 1000)] },
    ],
  };

  it('counts tracks, bytes and unavailable files per library', () => {
    expect(summarizeLibraries(catalog)).toEqual([
      { id: 'l1', name: 'One', count: 2, bytes: 1500, unavailable: 1 },
      { id: 'l2', name: 'Two', count: 1, bytes: 1000, unavailable: 0 },
    ]);
  });

  it('lists a file shared by two libraries once in the all-tracks view', () => {
    expect(uniqueLocalTracks(catalog.playlists.map((playlist) => playlist.tracks)).map((item) => item.id)).toEqual(['t1', 't2']);
  });
});

describe('formatting', () => {
  it('formats sizes', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
    expect(formatBytes(250 * 1024 * 1024)).toBe('250 MB');
    expect(formatBytes(Number.NaN)).toBe('0 B');
  });

  it('pluralises', () => {
    expect(pluralize(1, 'faixa', 'faixas')).toBe('1 faixa');
    expect(pluralize(0, 'faixa', 'faixas')).toBe('0 faixas');
  });
});

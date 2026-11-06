import { describe, expect, it } from 'vitest';
import type { LibraryItem } from '../models/library';
import { isTrackUnavailable, playableTracks, unavailableLabel, unavailableMessage, unavailableReasonOf, withUnavailable } from './track-availability';

const track = (patch: Partial<LibraryItem> = {}): LibraryItem => ({
  id: 't', title: 'Song', artist: 'A', album: 'B', duration: '3:00', source: 'youtube', accent: '#fff', ...patch,
});

describe('track availability', () => {
  it('treats a reason or a missing local file as unavailable', () => {
    expect(isTrackUnavailable(track())).toBe(false);
    expect(isTrackUnavailable(track({ available: true }))).toBe(false);
    expect(isTrackUnavailable(track({ available: false }))).toBe(true);
    expect(isTrackUnavailable(track({ unavailableReason: 'signed-out' }))).toBe(true);
    expect(isTrackUnavailable(null)).toBe(false);
  });

  it('derives the reason and the messages', () => {
    expect(unavailableReasonOf(track({ available: false }))).toBe('file-missing');
    expect(unavailableReasonOf(track({ unavailableReason: 'signed-out' }))).toBe('signed-out');
    expect(unavailableReasonOf(track())).toBeUndefined();
    expect(unavailableLabel('signed-out')).toContain('conta');
    expect(unavailableMessage(track({ unavailableReason: 'signed-out' }))).toContain('Conecte');
    expect(unavailableMessage(track({ available: false }))).toContain('Importe');
  });

  it('keeps order and drops only the unavailable tracks from a queue source', () => {
    const list = [track({ id: 'a' }), track({ id: 'b', unavailableReason: 'signed-out' }), track({ id: 'c' }), track({ id: 'd', available: false })];
    expect(playableTracks(list).map((item) => item.id)).toEqual(['a', 'c']);
    expect(playableTracks([])).toEqual([]);
  });

  it('adds and clears the reason without mutating the original', () => {
    const original = track();
    const marked = withUnavailable(original, 'signed-out');
    expect(marked.unavailableReason).toBe('signed-out');
    expect(original.unavailableReason).toBeUndefined();
    expect(withUnavailable(marked, undefined).unavailableReason).toBeUndefined();
    expect(withUnavailable(original, undefined)).toBe(original);
  });
});

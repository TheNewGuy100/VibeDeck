import { describe, expect, it } from 'vitest';
import { parsePersistedPlayerState, sameTrackByTitleAndArtist, shuffleTracks } from './player-state';

const track = { id: '1', title: 'Song', artist: 'Artist', album: 'Album', duration: '1:00', source: 'youtube' as const, accent: '#000' };

describe('player state helpers', () => {
  it('matches tracks by title and artist ignoring case and spacing', () => {
    expect(sameTrackByTitleAndArtist(track, { title: ' song ', artist: 'ARTIST' })).toBe(true);
    expect(sameTrackByTitleAndArtist(track, { ...track, artist: 'Another artist' })).toBe(false);
  });

  it('shuffles a copy without changing the source', () => {
    const source = ['a', 'b', 'c'];
    expect(shuffleTracks(source, () => 0)).toEqual(['b', 'c', 'a']);
    expect(source).toEqual(['a', 'b', 'c']);
  });

  it('rejects invalid persisted player state', () => {
    expect(parsePersistedPlayerState('{"track":{}}')).toBeNull();
    expect(parsePersistedPlayerState('not-json')).toBeNull();
  });
});

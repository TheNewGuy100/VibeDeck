import { describe, expect, it } from 'vitest';
import { isCurrentPlayerEndEvent } from '../interfaces/player';
import { advanceQueueContext, createQueueContext, createQueueMoveIntent, createSelectedTrackQueue, isCurrentPlayerRequest, manualQueueIndex, nextQueueIndex, nextRepeatMode, parsePersistedPlayerState, queueForRepeatTransition, queueIndexForTrack, sameTrackByTitleAndArtist, shuffleQueueContext, shuffleTracks } from './player-state';

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

  it('keeps queue ownership and index tied to the active playlist generation', () => {
    const tracks = ['a', 'b'];
    const context = createQueueContext('playlist-a', tracks, 3, 1);
    expect(context).toEqual({ playlistId: 'playlist-a', source: 'local', generation: 3, index: 1, tracks: ['a', 'b'], shuffled: false });
    expect(context.tracks).not.toBe(tracks);
  });

  it('creates a complete queue after playlist hydration', () => {
    const tracks = Array.from({ length: 250 }, (_, index) => `track-${index + 1}`);
    const context = createQueueContext('playlist-a', tracks, 7, 249);
    expect(context.tracks).toHaveLength(250);
    expect(context.index).toBe(249);
  });

  it('shuffles only an owned queue copy and keeps the current track first', () => {
    const display = ['a', 'b', 'c', 'd'];
    const queue = createQueueContext('playlist-a', display, 8, 2, 'youtube');
    const shuffled = shuffleQueueContext(queue, () => 0);
    expect(shuffled.tracks).toEqual(['c', 'b', 'd', 'a']);
    expect(shuffled.index).toBe(0);
    expect(shuffled.shuffled).toBe(true);
    expect(display).toEqual(['a', 'b', 'c', 'd']);
    expect(shuffled.tracks).not.toBe(display);
  });

  it('uses the selected-to-tail queue for off and repeat-one, and rotates the full playlist only for playlist repeat', () => {
    const source = ['a', 'b', 'c', 'd'];
    expect(createSelectedTrackQueue(source, 2, 'off')).toEqual({ tracks: ['c', 'd'], index: 0 });
    expect(createSelectedTrackQueue(source, 2, 'track')).toEqual({ tracks: ['c', 'd'], index: 0 });
    expect(createSelectedTrackQueue(source, 2, 'playlist')).toEqual({ tracks: ['c', 'd', 'a', 'b'], index: 0 });
    expect(source).toEqual(['a', 'b', 'c', 'd']);
  });

  it('keeps every visible playlist track and correct queue index for selections at first, middle and last positions', () => {
    const source = ['a', 'b', 'c', 'd'];
    for (const [selected, tail, rotated] of [
      [0, ['a', 'b', 'c', 'd'], ['a', 'b', 'c', 'd']],
      [2, ['c', 'd'], ['c', 'd', 'a', 'b']],
      [3, ['d'], ['d', 'a', 'b', 'c']],
    ] as const) {
      expect(createSelectedTrackQueue(source, selected, 'off')).toEqual({ tracks: tail, index: 0 });
      expect(createSelectedTrackQueue(source, selected, 'track')).toEqual({ tracks: tail, index: 0 });
      expect(createSelectedTrackQueue(source, selected, 'playlist')).toEqual({ tracks: rotated, index: 0 });
    }
  });

  it('resets a shuffled queue to source order when an explicit source track is selected', () => {
    const shuffled = shuffleQueueContext(createQueueContext('playlist-a', ['a', 'b', 'c', 'd'], 4, 0), () => 0);
    expect(shuffled.tracks).toEqual(['a', 'c', 'd', 'b']);
    expect(createSelectedTrackQueue(['a', 'b', 'c', 'd'], 1, 'track')).toEqual({ tracks: ['b', 'c', 'd'], index: 0 });
  });

  it('updates queue order and active index on consecutive explicit source selections', () => {
    const source = ['a', 'b', 'c', 'd'];
    const afterA = createSelectedTrackQueue(source, 0, 'off');
    expect(afterA).toEqual({ tracks: ['a', 'b', 'c', 'd'], index: 0 });
    const afterB = createSelectedTrackQueue(source, 1, 'off');
    expect(afterB).toEqual({ tracks: ['b', 'c', 'd'], index: 0 });
  });

  it('models repeat-track, playlist and off transitions at queue boundaries', () => {
    expect(nextRepeatMode('off')).toBe('playlist');
    expect(nextRepeatMode('playlist')).toBe('track');
    expect(nextRepeatMode('track')).toBe('off');
    expect(nextQueueIndex(1, 2, 'off')).toBeNull();
    expect(nextQueueIndex(1, 2, 'playlist')).toBe(0);
    expect(nextQueueIndex(1, 2, 'track')).toBe(1);
    expect(nextQueueIndex(0, 2, 'playlist')).toBe(1);
  });

  it('advances three selected-to-tail tracks in order and stops only after the tail', () => {
    const queue = createSelectedTrackQueue(['track-1', 'track-2', 'track-3'], 0, 'off');
    expect(queue.tracks).toEqual(['track-1', 'track-2', 'track-3']);
    const firstNext = nextQueueIndex(queue.index, queue.tracks.length, 'off');
    const secondNext = nextQueueIndex(firstNext!, queue.tracks.length, 'off');
    expect([queue.tracks[queue.index], queue.tracks[firstNext!], queue.tracks[secondNext!]]).toEqual(['track-1', 'track-2', 'track-3']);
    expect(nextQueueIndex(secondNext!, queue.tracks.length, 'off')).toBeNull();
  });

  it('activates five source tracks and advances 0→1→2→3→4 before stopping with repeat off', () => {
    const source = Array.from({ length: 5 }, (_, index) => `track-${index}`);
    const activation = createSelectedTrackQueue(source, 0, 'off');
    expect(activation.tracks).toEqual(source);
    expect(activation.index).toBe(0);
    const visited = [activation.tracks[activation.index]];
    let index = activation.index;
    while (true) {
      const next = nextQueueIndex(index, activation.tracks.length, 'off');
      if (next === null) break;
      index = next;
      visited.push(activation.tracks[index]);
    }
    expect(visited).toEqual(source);
    expect(index).toBe(4);
  });

  it('extends a partial queue after continued pagination before selecting its terminal tail', () => {
    const partial = ['track-0', 'track-1'];
    const continuation = ['track-2', 'track-3', 'track-4'];
    const hydrated = [...partial, ...continuation];
    const activation = createSelectedTrackQueue(hydrated, 1, 'off');
    expect(activation.tracks).toEqual(['track-1', 'track-2', 'track-3', 'track-4']);
    expect(activation.tracks.at(-1)).toBe('track-4');
    expect(nextQueueIndex(3, activation.tracks.length, 'off')).toBeNull();
  });

  it('rejects duplicate ended-event identity after queue index advances', () => {
    const active = { track: { ...track, id: 'track-2' }, queueGeneration: 7, queueIndex: 1, revision: 20, terminalBehavior: 'stop' as const };
    const duplicate = { trackId: 'track-1', queueGeneration: 7, queueIndex: 0, revision: 20, terminalBehavior: 'stop' as const };
    expect(isCurrentPlayerEndEvent(duplicate, active)).toBe(false);
  });

  it('repeat-one restarts the active track without reducing a five-track queue to a repeating short tail', () => {
    const queue = createSelectedTrackQueue(['a', 'b', 'c', 'd', 'e'], 0, 'track');
    expect(queue.tracks).toEqual(['a', 'b', 'c', 'd', 'e']);
    expect(nextQueueIndex(1, queue.tracks.length, 'track')).toBe(1);
    expect(manualQueueIndex(1, queue.tracks.length, 1, 'track')).toBe(2);
    expect(manualQueueIndex(4, queue.tracks.length, 1, 'track')).toBeNull();
  });

  it('keeps manual skip index aligned with selected five-track queue', () => {
    const queue = createSelectedTrackQueue(['a', 'b', 'c', 'd', 'e'], 0, 'off');
    const next = manualQueueIndex(queue.index, queue.tracks.length, 1, 'off');
    expect(next).toBe(1);
    expect(queue.tracks[next!]).toBe('b');
    expect(nextQueueIndex(next!, queue.tracks.length, 'off')).toBe(2);
  });

  it('preserves selected and advanced indices when queue entries have duplicate track IDs', () => {
    const queue = [{ id: 'a' }, { id: 'b' }, { id: 'a' }, { id: 'c' }];
    expect(queueIndexForTrack(queue, 2, 'a')).toBe(2);
    const next = manualQueueIndex(1, queue.length, 1, 'off');
    expect(next).toBe(2);
    expect(queueIndexForTrack(queue, next!, queue[next!].id)).toBe(2);
  });

  it('uses explicit terminal policies and a middle selection contains no prefix when repeat is off', () => {
    expect(createSelectedTrackQueue(['1', '2', '3'], 1, 'off')).toEqual({ tracks: ['2', '3'], index: 0 });
    expect(createSelectedTrackQueue(['1', '2', '3'], 1, 'playlist')).toEqual({ tracks: ['2', '3', '1'], index: 0 });
  });

  it('drops wrapped prefix when playlist repeat is disabled during playback', () => {
    const source = ['A', 'B', 'C', 'D'].map((id) => ({ id }));
    const wrapped = createSelectedTrackQueue(source, 1, 'playlist');
    expect(wrapped.tracks.map((item) => item.id)).toEqual(['B', 'C', 'D', 'A']);
    const switchedOff = queueForRepeatTransition(wrapped.tracks, 2, 'playlist', 'off', source, 'D');
    expect(switchedOff.tracks.map((item) => item.id)).toEqual(['D']);
    expect(switchedOff.index).toBe(0);
    expect(nextQueueIndex(0, switchedOff.tracks.length, 'off')).toBeNull();
  });

  it('restarts only the current queue entry for repeat-one and keeps manual next policy separate', () => {
    expect(nextQueueIndex(1, 3, 'track')).toBe(1);
    expect(manualQueueIndex(1, 3, 1, 'track')).toBe(2);
  });

  it('never wraps manual next/previous outside playlist-repeat mode', () => {
    expect(manualQueueIndex(2, 3, 1, 'off')).toBeNull();
    expect(manualQueueIndex(2, 3, 1, 'track')).toBeNull();
    expect(manualQueueIndex(0, 3, -1, 'off')).toBeNull();
    expect(manualQueueIndex(0, 3, -1, 'track')).toBeNull();
    expect(manualQueueIndex(2, 3, 1, 'playlist')).toBe(0);
    expect(manualQueueIndex(0, 3, -1, 'playlist')).toBe(2);
  });

  it('keeps sequential Next intents at 1→2 and 2→3 while the first activation is deferred', async () => {
    const queue = ['track-1', 'track-2', 'track-3'];
    let index = 0;
    let activationSequence = 0;
    let resolveFirst!: () => void;
    let resolveSecond!: () => void;
    const firstActivation = new Promise<void>((resolve) => { resolveFirst = resolve; });
    const secondActivation = new Promise<void>((resolve) => { resolveSecond = resolve; });
    const first = createQueueMoveIntent(index, queue.length, 1, 'off', activationSequence)!;
    index = first.toIndex;
    activationSequence = first.activationSequence;
    const firstCompletion = firstActivation.then(() => queue[first.toIndex]);
    expect([queue[first.fromIndex], queue[first.toIndex]]).toEqual(['track-1', 'track-2']);
    const second = createQueueMoveIntent(index, queue.length, 1, 'off', activationSequence)!;
    index = second.toIndex;
    activationSequence = second.activationSequence;
    const secondCompletion = secondActivation.then(() => queue[second.toIndex]);
    expect([queue[second.fromIndex], queue[second.toIndex]]).toEqual(['track-2', 'track-3']);
    resolveFirst();
    expect(await firstCompletion).toBe('track-2');
    resolveSecond();
    expect(await secondCompletion).toBe('track-3');
  });

  it('keeps the latest target after rapid intents and allows retry after activation failure', () => {
    const queue = ['track-1', 'track-2', 'track-3'];
    const first = createQueueMoveIntent(0, queue.length, 1, 'off', 10)!;
    const rapidSecond = createQueueMoveIntent(first.toIndex, queue.length, 1, 'off', first.activationSequence)!;
    expect([first.toIndex, rapidSecond.toIndex]).toEqual([1, 2]);
    expect(rapidSecond.activationSequence).toBeGreaterThan(first.activationSequence);
    const retry = createQueueMoveIntent(0, queue.length, 1, 'off', rapidSecond.activationSequence)!;
    expect(retry).toEqual({ fromIndex: 0, toIndex: 1, activationSequence: rapidSecond.activationSequence + 1 });
  });

  it('uses queue position, not duplicate track ID, as the Next transition identity', () => {
    const queue = [{ id: 'track-1' }, { id: 'duplicate' }, { id: 'duplicate' }];
    const first = createQueueMoveIntent(0, queue.length, 1, 'off', 0)!;
    const second = createQueueMoveIntent(first.toIndex, queue.length, 1, 'off', first.activationSequence)!;
    expect([queue[first.toIndex].id, queue[second.toIndex].id]).toEqual(['duplicate', 'duplicate']);
    expect([first.toIndex, second.toIndex]).toEqual([1, 2]);
  });

  it('keeps the queue and current index stable when repeat changes during playback', () => {
    const source = ['a', 'b', 'c', 'd'];
    expect(createSelectedTrackQueue(source, 2, 'off')).toEqual({ tracks: ['c', 'd'], index: 0 });
    expect(createSelectedTrackQueue(source, 2, 'track')).toEqual({ tracks: ['c', 'd'], index: 0 });
    expect(createSelectedTrackQueue(source, 2, 'playlist')).toEqual({ tracks: ['c', 'd', 'a', 'b'], index: 0 });
    expect(nextQueueIndex(1, 2, 'off')).toBeNull();
    expect(nextQueueIndex(1, 2, 'playlist')).toBe(0);
    expect(nextQueueIndex(0, 2, 'track')).toBe(0);
  });

  it('advances within the player queue regardless of the visible playlist', () => {
    const internalQueue = createQueueContext('playlist-a', ['old-a', 'old-b', 'old-c'], 9, 0, 'youtube');
    expect(advanceQueueContext(internalQueue, 1)).toMatchObject({ playlistId: 'playlist-a', index: 1, tracks: ['old-a', 'old-b', 'old-c'] });
    expect(advanceQueueContext({ ...internalQueue, index: 2 }, 1).index).toBe(0);
  });

  it('isolates queue tracks when switching playlists rapidly', () => {
    const first = createQueueContext('playlist-a', ['a'], 1);
    const second = createQueueContext('playlist-b', ['b'], 2);
    expect(second.playlistId).toBe('playlist-b');
    expect(second.tracks).toEqual(['b']);
    expect(second.tracks).not.toContain(first.tracks[0]);
  });

  it('rejects stale stream success after a rapid skip', () => {
    const oldRequest = { generation: 4, playlistId: 'playlist-a', trackId: 'track-a' };
    const currentRequest = { generation: 5, playlistId: 'playlist-a', trackId: 'track-b' };
    expect(isCurrentPlayerRequest(oldRequest, currentRequest)).toBe(false);
  });

  it('rejects stale stream errors and accepts recovery on the new track', () => {
    const oldRequest = { generation: 4, playlistId: 'playlist-a', trackId: 'track-a' };
    const currentRequest = { generation: 5, playlistId: 'playlist-a', trackId: 'track-b' };
    expect(isCurrentPlayerRequest(oldRequest, currentRequest)).toBe(false);
    expect(isCurrentPlayerRequest(currentRequest, currentRequest)).toBe(true);
  });
});

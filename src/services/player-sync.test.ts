import { describe, expect, it } from 'vitest';
import { applyPlayerSnapshot, isCurrentSeekIntent, reconcilePlayerSnapshot, seekSnapshotMatchesIntent, volumeSnapshotMatchesIntent, type NativeSeekIntent, type PlayerSnapshot, type VolumeIntent } from './player-sync';

const track = { id: 'track-a', title: 'Song', artist: 'Artist', album: 'Album', duration: '3:00', source: 'youtube' as const, accent: '#000' };
const snapshot = (revision: number, overrides: Partial<PlayerSnapshot> = {}): PlayerSnapshot => ({
  track,
  queue: [track],
  playlistId: 'playlist-a',
  queueSource: 'youtube',
  queueGeneration: 4,
  queueIndex: 0,
  shuffled: false,
  repeatMode: 'off' as const,
  terminalBehavior: 'stop' as const,
  isPlaying: true,
  position: 12,
  duration: 180,
  volume: 0.8,
  muted: false,
  volumeSequence: 1,
  source: 'youtube',
  nativeMode: true,
  revision,
  activationSequence: 1,
  error: null,
  ...overrides,
});

describe('native player rehydration contract', () => {
  it('hydrates an initial native snapshot', () => {
    const state = applyPlayerSnapshot({ snapshot: null, revision: -1, connection: 'connecting' }, snapshot(1));
    expect(state.snapshot?.track?.id).toBe('track-a');
    expect(state.connection).toBe('connected');
  });

  it('rehydrates the same snapshot after a reload without starting playback', () => {
    const first = applyPlayerSnapshot({ snapshot: null, revision: -1, connection: 'connecting' }, snapshot(5));
    const duplicate = applyPlayerSnapshot(first, snapshot(5));
    expect(duplicate).toBe(first);
  });

  it('accepts reconnect state after a bridge failure', () => {
    const state = applyPlayerSnapshot({ snapshot: null, revision: 5, connection: 'disconnected' }, snapshot(6, { position: 44 }));
    expect(state.snapshot?.position).toBe(44);
    expect(state.connection).toBe('connected');
  });

  it('reconnects at the same revision without replaying the track', () => {
    const state = applyPlayerSnapshot({ snapshot: null, revision: 5, connection: 'disconnected' }, snapshot(5));
    expect(state.connection).toBe('connected');
    expect(state.snapshot?.track?.id).toBe('track-a');
  });

  it('rejects stale snapshots', () => {
    const state = applyPlayerSnapshot({ snapshot: snapshot(8), revision: 8, connection: 'connected' }, snapshot(7, { position: 1 }));
    expect(state.snapshot?.position).toBe(12);
  });

  it('accepts a replacement track as a new revision and preserves queue identity', () => {
    const nextTrack = { ...track, id: 'track-b', title: 'Next' };
    const state = applyPlayerSnapshot({ snapshot: snapshot(9), revision: 9, connection: 'connected' }, snapshot(10, { track: nextTrack, queue: [track, nextTrack], queueIndex: 1 }));
    expect(state.snapshot?.track?.id).toBe('track-b');
    expect(state.snapshot?.queueIndex).toBe(1);
    expect(state.snapshot?.queue.map((item) => item.id)).toEqual(['track-a', 'track-b']);
  });

  it('restores a local queue without requiring the catalog to be loaded', () => {
    const local = { ...track, id: 'local-a', source: 'local' as const, managedPath: 'playlists/local/hash.mp3', available: true };
    const state = applyPlayerSnapshot({ snapshot: null, revision: -1, connection: 'connecting' }, snapshot(12, {
      track: local,
      queue: [local],
      source: 'local',
      playlistId: 'local-playlist',
    }));
    expect(state.snapshot?.queue[0].managedPath).toBe('playlists/local/hash.mp3');
  });

  it('restores queue order and shuffle state from a native snapshot after reload', () => {
    const reordered = [{ ...track, id: 'track-b' }, track];
    const state = applyPlayerSnapshot({ snapshot: null, revision: -1, connection: 'connecting' }, snapshot(13, {
      queue: reordered, queueSource: 'online', queueIndex: 1, shuffled: true,
    }));
    expect(state.snapshot?.queue.map((item) => item.id)).toEqual(['track-b', 'track-a']);
    expect(state.snapshot?.queueIndex).toBe(1);
    expect(state.snapshot?.shuffled).toBe(true);
  });

  it('retains the complete selected-to-tail queue and index across native refresh hydration', () => {
    const tail = Array.from({ length: 249 }, (_, index) => ({ ...track, id: `track-${index + 2}` }));
    const hydrated = applyPlayerSnapshot({ snapshot: null, revision: -1, connection: 'connecting' }, snapshot(14, {
      track: tail[0], queue: tail, queueIndex: 0, repeatMode: 'off',
    }));
    expect(hydrated.snapshot?.queue).toHaveLength(249);
    expect(hydrated.snapshot?.queue[0].id).toBe('track-2');
    expect(hydrated.snapshot?.queue.at(-1)?.id).toBe('track-250');
    expect(hydrated.snapshot?.queueIndex).toBe(0);
  });

  it('rejects a newer revision carrying an older queue generation', () => {
    const current = snapshot(20, { queueGeneration: 8 });
    const state = applyPlayerSnapshot({ snapshot: current, revision: 20, connection: 'connected' }, snapshot(21, { queueGeneration: 7, position: 90 }));
    expect(state.snapshot).toBe(current);
    expect(state.snapshot?.position).toBe(12);
  });

  it('rejects a same-generation native snapshot that truncates an active queue', () => {
    const queue = Array.from({ length: 5 }, (_, index) => ({ ...track, id: `track-${index + 1}` }));
    const current = snapshot(30, { queue, queueIndex: 2, track: queue[2] });
    const state = applyPlayerSnapshot({ snapshot: current, revision: 30, connection: 'connected' }, snapshot(31, {
      queue: queue.slice(0, 3), queueIndex: 2, track: queue[2],
    }));
    expect(state.snapshot).toBe(current);
    expect(state.snapshot?.queue).toHaveLength(5);
  });

  it('does not allow a snapshot from before a source selection to revert the live queue', () => {
    const nativeState = { snapshot: snapshot(20, { queueGeneration: 8 }), revision: 20, connection: 'connected' as const };
    const selectedQueue = snapshot(20, { queueGeneration: 9, queue: [{ ...track, id: 'track-b' }], queueIndex: 0 });
    const stateWithLocalIntent = { ...nativeState, snapshot: selectedQueue };
    const reconciled = reconcilePlayerSnapshot(stateWithLocalIntent, snapshot(21, { queueGeneration: 8 }), 9, true);
    expect(reconciled).toBe(stateWithLocalIntent);
    expect(reconciled.snapshot?.queue.map((item) => item.id)).toEqual(['track-b']);
  });

  it('accepts the native queue once its activation reaches the requested generation', () => {
    const nativeState = { snapshot: snapshot(20, { queueGeneration: 8 }), revision: 20, connection: 'connected' as const };
    const activated = snapshot(21, { queueGeneration: 9, queue: [{ ...track, id: 'track-b' }] });
    expect(reconcilePlayerSnapshot(nativeState, activated, 9, true).snapshot).toBe(activated);
  });

  it('rejects an old same-generation stop snapshot while the next activation is pending', () => {
    const current = snapshot(20, { queueIndex: 1, queueGeneration: 4, activationSequence: 7 });
    const state = { snapshot: current, revision: 20, connection: 'connected' as const };
    const staleStop = snapshot(21, { queueIndex: 1, queueGeneration: 4, activationSequence: 7 });
    expect(reconcilePlayerSnapshot(state, staleStop, 4, true, 8)).toBe(state);
    const activated = snapshot(22, { queueIndex: 2, queueGeneration: 4, activationSequence: 8 });
    expect(reconcilePlayerSnapshot(state, activated, 4, true, 8).snapshot).toBe(activated);
  });

  it('represents no active player explicitly', () => {
    const state = applyPlayerSnapshot({ snapshot: snapshot(10), revision: 10, connection: 'connected' }, snapshot(11, { track: null, playlistId: null, nativeMode: false, isPlaying: false, source: null }));
    expect(state.snapshot?.track).toBeNull();
    expect(state.snapshot?.nativeMode).toBe(false);
  });
});

describe('native seek contract', () => {
  const intent = (overrides: Partial<NativeSeekIntent> = {}): NativeSeekIntent => ({
    trackId: 'track-a', queueGeneration: 4, seconds: 90, baseRevision: 10, sequence: 1, ...overrides,
  });

  it('accepts the authoritative response for the active track and revision', () => {
    expect(seekSnapshotMatchesIntent(snapshot(11, { position: 90 }), intent())).toBe(true);
  });

  it('keeps only the newest repeated seek intent current', () => {
    const first = intent();
    const latest = intent({ seconds: 120, sequence: 2 });
    expect(isCurrentSeekIntent(first, latest)).toBe(false);
    expect(isCurrentSeekIntent(latest, latest)).toBe(true);
  });

  it('rejects a response from an older native revision', () => {
    expect(seekSnapshotMatchesIntent(snapshot(10, { position: 90 }), intent())).toBe(false);
  });

  it('rejects a response after the active track is replaced', () => {
    expect(seekSnapshotMatchesIntent(snapshot(11, { track: { ...track, id: 'track-b' } }), intent())).toBe(false);
  });

  it('rejects a response when there is no active native player', () => {
    expect(seekSnapshotMatchesIntent(snapshot(11, { nativeMode: false, track: null }), intent())).toBe(false);
  });

  it('accepts rehydration after seek when it belongs to the same track generation', () => {
    expect(seekSnapshotMatchesIntent(snapshot(15, { position: 90 }), intent({ baseRevision: 12 }))).toBe(true);
  });
});

describe('native volume contract', () => {
  const intent = (overrides: Partial<VolumeIntent> = {}): VolumeIntent => ({ volume: 0.4, muted: false, sequence: 3, ...overrides });

  it('accepts the newest rapid volume intent only', () => {
    expect(volumeSnapshotMatchesIntent(snapshot(12, { volume: 0.9, volumeSequence: 4 }), intent({ sequence: 4 }))).toBe(true);
    expect(volumeSnapshotMatchesIntent(snapshot(13, { volume: 0.2, volumeSequence: 2 }), intent({ sequence: 4 }))).toBe(false);
  });

  it('keeps mute and unmute as ordered intents', () => {
    expect(volumeSnapshotMatchesIntent(snapshot(14, { muted: true, volume: 0, volumeSequence: 5 }), intent({ muted: true, sequence: 5 }))).toBe(true);
    expect(volumeSnapshotMatchesIntent(snapshot(15, { muted: false, volume: 0.4, volumeSequence: 6 }), intent({ sequence: 6 }))).toBe(true);
  });

  it('accepts native volume state during reconnect hydration', () => {
    const hydrated = applyPlayerSnapshot({ snapshot: null, revision: -1, connection: 'connecting' }, snapshot(20, { volume: 0.25, volumeSequence: 8 }));
    expect(hydrated.snapshot?.volumeSequence).toBe(8);
    expect(hydrated.snapshot?.volume).toBe(0.25);
  });

  it('does not treat an unavailable native response as a browser fallback failure', () => {
    const browserSnapshot = snapshot(1, { nativeMode: false, track: null, volumeSequence: 0 });
    expect(volumeSnapshotMatchesIntent(browserSnapshot, intent({ sequence: 1 }))).toBe(false);
    expect(browserSnapshot.nativeMode).toBe(false);
  });
});

import { describe, expect, it } from 'vitest';
import { bridgeErrorMessage, normalizeBridgeError } from './bridge';
import { isManagedTrackReference } from './library';
import { isCurrentPlayerEndEvent, isPlayerStateEvent, parsePlayerSnapshot, terminalBehaviorForRepeatMode } from './player';
import { isAnalyzerEvent, parseAnalyzerSnapshot } from './visualizer';

const track = { id: 'track-a', title: 'Song', artist: 'Artist', album: 'Album', duration: '3:00', source: 'youtube' as const, accent: '#000' };
const validSnapshot = { track, queue: [track], playlistId: 'playlist-a', queueSource: 'youtube', queueGeneration: 2, queueIndex: 0, shuffled: false, repeatMode: 'off' as const, terminalBehavior: 'stop' as const, isPlaying: true, position: 1, duration: 180, volume: 1, muted: false, source: 'youtube', nativeMode: true, revision: 4, error: null };

describe('bridge contracts', () => {
  it('parses a player snapshot without changing serde camelCase payloads', () => {
    expect(parsePlayerSnapshot(validSnapshot).queueGeneration).toBe(2);
    expect(() => parsePlayerSnapshot({ revision: '4', queue: [] })).toThrow('snapshot');
  });

  it('discriminates player and analyzer events', () => {
    expect(isPlayerStateEvent({ kind: 'ended', snapshot: validSnapshot })).toBe(true);
    expect(isPlayerStateEvent({ kind: 'unknown', snapshot: validSnapshot })).toBe(false);
    expect(isAnalyzerEvent({ kind: 'levelsUpdated', snapshot: { levels: [0.1] } })).toBe(true);
    expect(parseAnalyzerSnapshot([0.2]).levels).toEqual([0.2]);
  });

  it('maps repeat mode to terminal policy and rejects stale end identities', () => {
    expect(terminalBehaviorForRepeatMode('off')).toBe('stop');
    expect(terminalBehaviorForRepeatMode('playlist')).toBe('wrapToStart');
    expect(terminalBehaviorForRepeatMode('track')).toBe('restartCurrent');
    const end = { trackId: 'track-a', queueGeneration: 2, queueIndex: 0, revision: 4, terminalBehavior: 'stop' as const };
    expect(isCurrentPlayerEndEvent(end, validSnapshot)).toBe(true);
    expect(isCurrentPlayerEndEvent({ ...end, queueGeneration: 1 }, validSnapshot)).toBe(false);
    expect(isCurrentPlayerEndEvent({ ...end, trackId: 'track-old' }, validSnapshot)).toBe(false);
  });

  it('normalizes structured and plain native errors', () => {
    expect(normalizeBridgeError('{"code":"timeout","message":"Timed out","retryable":true}', 'load').retryable).toBe(true);
    expect(bridgeErrorMessage(new Error('offline'), 'load')).toBe('offline');
    expect(normalizeBridgeError({ code: 'missingFile', message: 'missing', correlationId: 'corr-1' }, 'play_local_audio', 'fallback').correlationId).toBe('corr-1');
  });
});

describe('local catalog contracts', () => {
  it('accepts managed references and rejects browser-only paths', () => {
    expect(isManagedTrackReference({ id: 'local-a', managedPath: 'playlists/p/a.mp3', contentHash: 'hash', byteLength: 4, extension: 'mp3', fileName: 'a.mp3', available: true })).toBe(true);
    expect(isManagedTrackReference({ id: 'local-a', managedPath: 'C:/Music/a.mp3' })).toBe(false);
  });
});

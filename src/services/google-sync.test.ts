import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen }));
vi.mock('./secure-storage', () => ({ decryptSecureRecord: vi.fn(), encryptSecureRecord: vi.fn() }));

import { GoogleSyncService } from './google-sync';

describe('GoogleSyncService bridge', () => {
  beforeEach(() => invoke.mockReset());

  it('loads the initial playlist page and continuation pages through the expected commands', async () => {
    const service = new GoogleSyncService();
    invoke
      .mockResolvedValueOnce({ tracks: [{ id: 'first' }], hasMore: true })
      .mockResolvedValueOnce({ tracks: [{ id: 'second' }], hasMore: false });

    await expect(service.loadYouTubePlaylistTracks('VLPL123')).resolves.toEqual({ items: [{ id: 'first' }], hasMore: true });
    await expect(service.loadYouTubePlaylistMoreTracks('VLPL123')).resolves.toEqual({ items: [{ id: 'second' }], hasMore: false });
    expect(invoke).toHaveBeenNthCalledWith(1, 'load_youtube_playlist_tracks', { playlistId: 'VLPL123' });
    expect(invoke).toHaveBeenNthCalledWith(2, 'load_youtube_playlist_more_tracks', { playlistId: 'VLPL123' });
  });

  it('reconnects to native audio state without starting a new stream', async () => {
    const service = new GoogleSyncService();
    invoke
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(123.5)
      .mockResolvedValueOnce(240)
      .mockResolvedValueOnce(false);

    await expect(service.isAudioActive()).resolves.toBe(true);
    await expect(service.getAudioPosition()).resolves.toBe(123.5);
    await expect(service.getAudioDuration()).resolves.toBe(240);
    await expect(service.isAudioPaused()).resolves.toBe(false);
    expect(invoke).not.toHaveBeenCalledWith('play_audio_url', expect.anything());
  });

  it('sends a revisioned seek intent and returns the authoritative snapshot', async () => {
    const service = new GoogleSyncService();
    const response = { revision: 9, position: 90 };
    invoke.mockResolvedValue(response);
    const request = { trackId: 'track-a', queueGeneration: 3, seconds: 90, baseRevision: 8, sequence: 2, revision: 8 };
    await expect(service.seekAudio(request)).resolves.toEqual(response);
    expect(invoke).toHaveBeenCalledWith('seek_audio', { request });
  });

  it('sends a sequenced volume intent and returns the authoritative snapshot', async () => {
    const service = new GoogleSyncService();
    const response = { revision: 10, volume: 0.4, muted: false, volumeSequence: 7 };
    invoke.mockResolvedValue(response);
    const intent = { volume: 0.4, muted: false, sequence: 7 };
    await expect(service.setAudioVolume(intent)).resolves.toEqual(response);
    expect(invoke).toHaveBeenCalledWith('set_audio_volume', { request: intent });
  });

  it('does not start playback when a stream request is aborted while resolving', async () => {
    const service = new GoogleSyncService();
    (service as unknown as { youtubeSession: boolean }).youtubeSession = true;
    const controller = new AbortController();
    let resolveBridge!: (value: unknown) => void;
    invoke.mockImplementationOnce(() => new Promise((resolve) => { resolveBridge = resolve; }));

    const request = service.playAudio('old-track', { track: { id: 'old-track', title: 'Old', artist: 'Artist', album: 'Album', duration: '--:--', source: 'youtube', accent: '#000' }, queue: [], playlistId: 'playlist', queueSource: 'youtube', shuffled: false, queueGeneration: 1, queueIndex: 0, activationSequence: 1 }, controller.signal);
    controller.abort();
    resolveBridge({ url: 'https://example.test/old', mimeType: 'audio/mp4' });

    await expect(request).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoke).not.toHaveBeenCalledWith('play_audio_url', expect.anything());
  });

  it('rejects an already obsolete stream before invoking native playback', async () => {
    const service = new GoogleSyncService();
    (service as unknown as { youtubeSession: boolean }).youtubeSession = true;
    const controller = new AbortController();
    controller.abort();

    await expect(service.playAudio('obsolete-track', { track: { id: 'obsolete-track', title: 'Obsolete', artist: 'Artist', album: 'Album', duration: '--:--', source: 'youtube', accent: '#000' }, queue: [], playlistId: 'playlist', queueSource: 'youtube', shuffled: false, queueGeneration: 1, queueIndex: 0, activationSequence: 1 }, controller.signal)).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoke).not.toHaveBeenCalled();
  });
});

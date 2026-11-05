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

  it('reads the safe native audio diagnostic without issuing playback or YouTube requests', async () => {
    const service = new GoogleSyncService();
    const diagnostic = { playerActivationSequence: 12, errorCode: 'codec_error', mediaBytes: 64 };
    invoke.mockResolvedValueOnce(diagnostic);

    await expect(service.getAudioPlaybackDiagnostic()).resolves.toEqual(diagnostic);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('get_audio_playback_diagnostic');
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

  it('ignores completion of an obsolete native continuous activation', async () => {
    const service = new GoogleSyncService();
    (service as unknown as { youtubeSession: boolean }).youtubeSession = true;
    const controller = new AbortController();
    let resolveBridge!: (value: unknown) => void;
    invoke.mockImplementationOnce(() => new Promise((resolve) => { resolveBridge = resolve; }));

    const request = service.playAudio('old-track', { track: { id: 'old-track', title: 'Old', artist: 'Artist', album: 'Album', duration: '--:--', source: 'youtube', accent: '#000' }, queue: [], playlistId: 'playlist', queueSource: 'youtube', shuffled: false, queueGeneration: 1, queueIndex: 0, activationSequence: 1 }, controller.signal);
    controller.abort();
    resolveBridge({ scope: 'continuous_sabr_relay', audioEngineStarted: true, continuation: true, seek: false });

    await expect(request).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoke).toHaveBeenCalledWith('play_youtube_audio_ump_stream', expect.objectContaining({ videoId: 'old-track' }));
  });

  it('surfaces structured native UMP handoff failures as actionable messages', async () => {
    const service = new GoogleSyncService();
    (service as unknown as { youtubeSession: boolean }).youtubeSession = true;
    invoke.mockRejectedValueOnce(JSON.stringify({
        code: 'audio_stream_url_body',
        operation: 'play_youtube_audio_ump_stream',
        message: 'O YouTube Music retornou uma URL como texto no lugar dos bytes de áudio. Tente novamente.',
        correlationId: 'audio-stream',
        retryable: true,
      }));

    await expect(service.playAudio('track-a', {
      track: { id: 'track-a', title: 'Track', artist: 'Artist', album: 'Album', duration: '--:--', source: 'youtube', accent: '#000' },
      queue: [], playlistId: 'playlist', queueSource: 'youtube', shuffled: false,
      queueGeneration: 1, queueIndex: 0, activationSequence: 1,
    })).rejects.toMatchObject({
      message: 'O YouTube Music retornou uma URL como texto no lugar dos bytes de áudio. Tente novamente.',
    });
    expect(invoke).toHaveBeenCalledWith('play_youtube_audio_ump_stream', expect.objectContaining({ videoId: 'track-a' }));
  });

  it('invokes native playback with the full activation and reports continuous relay scope', async () => {
    const service = new GoogleSyncService();
    (service as unknown as { youtubeSession: boolean }).youtubeSession = true;
    const activation = {
      track: { id: 'track-a', title: 'Track', artist: 'Artist', album: 'Album', duration: '--:--', durationSeconds: 193, source: 'youtube' as const, accent: '#000' },
      queue: [], playlistId: 'playlist', queueSource: 'youtube' as const, shuffled: false,
      queueGeneration: 4, queueIndex: 2, activationSequence: 9,
    };
    invoke.mockResolvedValueOnce({ scope: 'continuous_sabr_relay', audioEngineStarted: true, continuation: true, seek: false });

    await expect(service.playAudio('track-a', activation)).resolves.toEqual({ durationSeconds: 193 });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith('play_youtube_audio_ump_stream', { videoId: 'track-a', activation });
  });

  const activationFor = (id: string) => ({
    track: { id, title: 'Track', artist: 'Artist', album: 'Album', duration: '--:--', durationSeconds: 200, source: 'youtube' as const, accent: '#000' },
    queue: [], playlistId: 'playlist', queueSource: 'youtube' as const, shuffled: false,
    queueGeneration: 1, queueIndex: 0, activationSequence: 3,
  });

  it('plays through the native byte-range player when it is enabled and never touches the relay', async () => {
    const service = new GoogleSyncService();
    service.useNativeRange(true);
    invoke.mockResolvedValueOnce({ scope: 'native_byte_range', audioEngineStarted: true, continuation: true, seek: true });

    await expect(service.playAudio('track-n', activationFor('track-n'))).resolves.toEqual({ durationSeconds: 200 });
    expect(invoke).toHaveBeenCalledExactlyOnceWith('play_youtube_audio_native_range', { videoId: 'track-n', activation: activationFor('track-n') });
  });

  it('falls back to the relay when the native player fails to start', async () => {
    const service = new GoogleSyncService();
    service.useNativeRange(true);
    invoke
      .mockRejectedValueOnce('native_range_header_incomplete')
      .mockResolvedValueOnce({ scope: 'continuous_sabr_relay', audioEngineStarted: true, continuation: true, seek: false });

    await expect(service.playAudio('track-f', activationFor('track-f'))).resolves.toEqual({ durationSeconds: 200 });
    expect(invoke).toHaveBeenNthCalledWith(1, 'play_youtube_audio_native_range', expect.anything());
    expect(invoke).toHaveBeenNthCalledWith(2, 'play_youtube_audio_ump_stream', expect.anything());
  });

  it('stays aborted instead of falling back when the activation was superseded during native startup', async () => {
    const service = new GoogleSyncService();
    service.useNativeRange(true);
    const controller = new AbortController();
    invoke.mockImplementationOnce(() => { controller.abort(); return Promise.reject('native_range_superseded'); });

    await expect(service.playAudio('track-s', activationFor('track-s'), controller.signal)).rejects.toMatchObject({ name: 'AbortError' });
    expect(invoke).toHaveBeenCalledExactlyOnceWith('play_youtube_audio_native_range', expect.anything());
  });

  it('reports native range availability as false when the command is missing', async () => {
    const service = new GoogleSyncService();
    invoke.mockRejectedValueOnce('command not found');
    await expect(service.isNativeRangeAvailable()).resolves.toBe(false);
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

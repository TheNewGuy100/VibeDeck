import { beforeEach, describe, expect, it, vi } from 'vitest';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('./secure-storage', () => ({ decryptSecureRecord: vi.fn(), encryptSecureRecord: vi.fn() }));

import { GoogleSyncService } from './google-sync';

describe('GoogleSyncService bridge', () => {
  beforeEach(() => invoke.mockReset());

  it('loads the initial playlist page and continuation pages through the expected commands', async () => {
    const service = new GoogleSyncService();
    invoke
      .mockResolvedValueOnce([{ id: 'first' }])
      .mockResolvedValueOnce([{ id: 'second' }]);

    await expect(service.loadYouTubePlaylistTracks('VLPL123')).resolves.toEqual([{ id: 'first' }]);
    await expect(service.loadYouTubePlaylistMoreTracks('VLPL123')).resolves.toEqual([{ id: 'second' }]);
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
});

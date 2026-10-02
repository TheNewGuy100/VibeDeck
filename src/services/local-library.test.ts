import { beforeEach, describe, expect, it, vi } from 'vitest';
import { extractNativePaths, LocalLibraryService, localPlaybackErrorMessage, localPlaybackLogEntry, localPlaylistToPlaylist, requireNativePaths, selectedDialogPaths, shouldApplyLocalOperation, summarizeLocalImport } from './local-library';

const invoke = vi.hoisted(() => vi.fn());
const open = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open }));

describe('local library bridge', () => {
  beforeEach(() => {
    invoke.mockReset();
    open.mockReset();
  });

  it.each([
    [['C:/Music/a.mp3', 'C:/Music/a.mp3'], ['C:/Music/a.mp3']],
    [{ paths: ['C:/Music/a.mp3'] }, ['C:/Music/a.mp3']],
    [{ payload: { paths: ['C:/Music/folder'] } }, ['C:/Music/folder']],
    ['C:/Music/a.mp3', ['C:/Music/a.mp3']],
  ])('extracts native drag/drop payload variant %#', (payload, expected) => {
    expect(extractNativePaths(payload as never)).toEqual(expected);
  });

  it('reports a usable error when native payload has no path', () => {
    expect(() => requireNativePaths({ paths: [] })).toThrow('caminho utilizável');
  });

  it('keeps typed native playback errors actionable in the UI', () => {
    expect(localPlaybackErrorMessage('{"code":"missingFile"}')).toContain('missingFile');
    expect(localPlaybackErrorMessage('{"code":"decoderFailure","detail":"AAC inválido"}')).toBe('AAC inválido');
    expect(localPlaybackErrorMessage({ code: 'invalidContainer', message: 'container', correlationId: 'corr-2' })).toContain('invalidContainer');
    expect(localPlaybackErrorMessage({ code: 'noOutputDevice', message: 'device', correlationId: 'corr-3' })).toContain('noOutputDevice');
  });

  it('creates a structured UI log entry without stringifying the native error object', () => {
    expect(localPlaybackLogEntry({ code: 'decoderFailure', message: 'Falha no decoder', correlationId: 'corr-4', retryable: false })).toEqual({
      message: 'Falha no decoder', operation: 'play_local_audio', correlationId: 'corr-4', level: 'error', code: 'decoderFailure', details: undefined,
    });
  });

  it('keeps replacement payloads isolated and allows recovery on another track', async () => {
    invoke.mockResolvedValueOnce({ nativeMode: true, isPlaying: true, track: { id: 'local-a' }, queueGeneration: 4 })
      .mockResolvedValueOnce({ nativeMode: true, isPlaying: true, track: { id: 'local-b' }, queueGeneration: 5 });
    const service = new LocalLibraryService();
    await service.playTrack('local-a', 'playlists/p/a.mp3', { track: { id: 'local-a', title: 'A', artist: 'A', album: 'A', duration: '--:--', source: 'local', accent: '#000' }, queue: [], playlistId: 'p', queueSource: 'local', shuffled: false, queueGeneration: 4, queueIndex: 0, activationSequence: 1 });
    await service.playTrack('local-b', 'playlists/p/b.mp3', { track: { id: 'local-b', title: 'B', artist: 'B', album: 'B', duration: '--:--', source: 'local', accent: '#000' }, queue: [], playlistId: 'p', queueSource: 'local', shuffled: false, queueGeneration: 5, queueIndex: 1, activationSequence: 2 });
    expect(invoke).toHaveBeenNthCalledWith(2, 'play_local_audio', expect.objectContaining({ request: expect.objectContaining({ trackId: 'local-b', managedPath: 'playlists/p/b.mp3' }) }));
  });

  it('sends the native import request without File objects', async () => {
    invoke.mockResolvedValue({ operationId: 'op-1', files: [], cancelled: false, catalog: { version: 1, playlists: [] } });
    await new LocalLibraryService().importPaths({ paths: ['C:/Music/a.mp3'], playlistName: 'Biblioteca local', operationId: 'op-1' });
    expect(invoke).toHaveBeenCalledWith('import_local_paths', { request: { paths: ['C:/Music/a.mp3'], playlistName: 'Biblioteca local', operationId: 'op-1' } });
  });

  it('starts persisted playback by catalog id and managed path', async () => {
    invoke.mockResolvedValue({ nativeMode: true, isPlaying: true, track: { id: 'local-a' }, queueGeneration: 1 });
    await new LocalLibraryService().playTrack('local-a', 'playlists/local-x/hash.mp3', { track: { id: 'local-a', title: 'Local', artist: 'Artist', album: 'Album', duration: '--:--', source: 'local', accent: '#000' }, queue: [], playlistId: 'local-x', queueSource: 'local', shuffled: false, queueGeneration: 1, queueIndex: 0, activationSequence: 1 });
    expect(invoke).toHaveBeenCalledWith('play_local_audio', {
      request: { trackId: 'local-a', managedPath: 'playlists/local-x/hash.mp3', correlationId: expect.stringMatching(/^local-playback-/) },
      activation: { track: { id: 'local-a', title: 'Local', artist: 'Artist', album: 'Album', duration: '--:--', source: 'local', accent: '#000' }, queue: [], playlistId: 'local-x', queueSource: 'local', shuffled: false, queueGeneration: 1, queueIndex: 0, activationSequence: 1 },
    });
  });

  it('does not accept an activation snapshot before native audio output is playing', async () => {
    invoke.mockResolvedValue({ nativeMode: true, isPlaying: false, track: { id: 'local-a' }, queueGeneration: 1 });
    await expect(new LocalLibraryService().playTrack('local-a', 'playlists/local-x/hash.mp3', { track: { id: 'local-a', title: 'Local', artist: 'Artist', album: 'Album', duration: '--:--', source: 'local', accent: '#000' }, queue: [], playlistId: 'local-x', queueSource: 'local', shuffled: false, queueGeneration: 1, queueIndex: 0, activationSequence: 1 }))
      .rejects.toMatchObject({ code: 'activation_not_confirmed', operation: 'play_local_audio' });
  });

  it('normalizes native local failures with operation and correlation', async () => {
    invoke.mockRejectedValue({ code: 'missingFile', message: 'missing', correlationId: 'corr-1', retryable: false });
    await expect(new LocalLibraryService().playTrack('local-a', 'playlists/local-x/hash.mp3', { track: { id: 'local-a', title: 'Local', artist: 'Artist', album: 'Album', duration: '--:--', source: 'local', accent: '#000' }, queue: [], playlistId: 'local-x', queueSource: 'local', shuffled: false, queueGeneration: 1, queueIndex: 0, activationSequence: 1 })).rejects.toMatchObject({ code: 'missingFile', operation: 'play_local_audio', correlationId: 'corr-1', retryable: false });
  });

  it('rejects a stale or mismatched activation before crossing the native bridge', async () => {
    const service = new LocalLibraryService();
    await expect(service.playTrack('local-a', 'playlists/local-x/hash.mp3', {
      track: { id: 'other', title: 'Other', artist: 'Artist', album: 'Album', duration: '--:--', source: 'local', accent: '#000' },
      queue: [], playlistId: 'local-x', queueSource: 'local', shuffled: false, queueGeneration: 2, queueIndex: 0, activationSequence: 1,
    })).rejects.toThrow('não corresponde');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('opens native file and folder selectors with distinct options', async () => {
    open.mockResolvedValueOnce('C:/Music/song.mp3').mockResolvedValueOnce(['C:/Music/Album']);
    const service = new LocalLibraryService();
    expect(selectedDialogPaths(await service.pickPaths(false))).toEqual(['C:/Music/song.mp3']);
    expect(selectedDialogPaths(await service.pickPaths(true))).toEqual(['C:/Music/Album']);
    expect(open).toHaveBeenNthCalledWith(1, { directory: false, multiple: true, recursive: false });
    expect(open).toHaveBeenNthCalledWith(2, { directory: true, multiple: true, recursive: true });
  });

  it('treats dialog cancellation as an empty selection', () => {
    expect(selectedDialogPaths(null)).toEqual([]);
  });

  it('maps managed tracks to metadata-only library items', () => {
    const playlist = localPlaylistToPlaylist({ id: 'local-x', name: 'Mix', tracks: [{ id: 'local-a', contentHash: 'hash', byteLength: 4, extension: 'mp3', fileName: 'a.mp3', managedPath: 'playlists/local-x/hash.mp3', available: false }] });
    expect(playlist.tracks[0]).toMatchObject({ id: 'local-a', managedPath: 'playlists/local-x/hash.mp3', available: false });
    expect(playlist.tracks[0]).not.toHaveProperty('audioUrl');
  });

  it('rejects stale operation generations and classifies partial results', () => {
    expect(shouldApplyLocalOperation('op-1', 2, 'op-1', 1)).toBe(false);
    expect(shouldApplyLocalOperation('op-1', 2, 'op-2', 2)).toBe(false);
    expect(shouldApplyLocalOperation('op-1', 2, 'op-1', 2)).toBe(true);
    expect(summarizeLocalImport([
      { path: 'a.mp3', status: 'imported' },
      { path: 'b.mp3', status: 'skipped', message: 'Duplicate content' },
      { path: 'c.exe', status: 'unsupported' },
      { path: 'd.mp3', status: 'failed', message: 'I/O' },
    ])).toEqual({ imported: 1, duplicates: 1, unsupported: 1, failed: 1, cancelled: 0 });
  });
});

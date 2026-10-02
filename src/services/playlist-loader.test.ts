import { describe, expect, it, vi } from 'vitest';
import { isCurrentPlaylistRequest, loadAllPages, PlaylistOperationRegistry, shouldRevealPlaylistTracks } from './playlist-loader';

describe('playlist track visibility', () => {
  it('keeps first-page tracks hidden while loading and reveals them at terminal or error', () => {
    expect(shouldRevealPlaylistTracks('loading')).toBe(false);
    expect(shouldRevealPlaylistTracks('complete')).toBe(true);
    expect(shouldRevealPlaylistTracks('error')).toBe(true);
    expect(shouldRevealPlaylistTracks('cancelled')).toBe(true);
    expect(shouldRevealPlaylistTracks('stale')).toBe(true);
  });
});

describe('loadAllPages', () => {
  it('loads every page and removes duplicate items', async () => {
    const next = vi.fn()
      .mockResolvedValueOnce([{ id: '2' }, { id: '3' }])
      .mockResolvedValueOnce([{ id: '3' }, { id: '4' }])
      .mockResolvedValueOnce([]);

    await expect(loadAllPages([{ id: '1' }], next, (item) => item.id)).resolves.toEqual([
      { id: '1' }, { id: '2' }, { id: '3' }, { id: '4' },
    ]);
    expect(next).toHaveBeenCalledTimes(3);
  });

  it('stops when a provider repeats a page', async () => {
    const next = vi.fn().mockResolvedValue([{ id: '1' }]);
    await expect(loadAllPages([{ id: '1' }], next, (item) => item.id)).resolves.toEqual([{ id: '1' }]);
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('keeps a terminal first page without requesting a continuation', async () => {
    const next = vi.fn();
    const onStop = vi.fn();

    await expect(loadAllPages({ items: [{ id: '1' }], hasMore: false }, next, (item) => item.id, { onStop })).resolves.toEqual([
      { id: '1' },
    ]);
    expect(next).not.toHaveBeenCalled();
    expect(onStop).toHaveBeenLastCalledWith({ reason: 'provider-terminal', pagesLoaded: 0 });
  });

  it('continues loading when a page advertises another continuation', async () => {
    const next = vi.fn()
      .mockResolvedValueOnce({ items: [{ id: '2' }], hasMore: true, continuation: 'page-3' })
      .mockResolvedValueOnce({ items: [{ id: '3' }], hasMore: false, continuation: null });

    await expect(loadAllPages([{ id: '1' }], next, (item) => item.id)).resolves.toEqual([
      { id: '1' }, { id: '2' }, { id: '3' },
    ]);
    expect(next).toHaveBeenCalledTimes(2);
  });

  it('loads 250 tracks across 100/100/50 pages without a first-page cap', async () => {
    const tracks = Array.from({ length: 250 }, (_, index) => ({ id: `track-${index + 1}` }));
    const next = vi.fn()
      .mockResolvedValueOnce({ items: tracks.slice(100, 200), hasMore: true, continuation: 'page-3' })
      .mockResolvedValueOnce({ items: tracks.slice(200), hasMore: false, continuation: null });

    await expect(loadAllPages({ items: tracks.slice(0, 100), hasMore: true, continuation: 'page-2' }, next, (item) => item.id)).resolves.toHaveLength(250);
    expect(next).toHaveBeenCalledTimes(2);
  });

  it('loads a 629-track playlist across six full pages and a final partial page', async () => {
    const tracks = Array.from({ length: 629 }, (_, index) => ({ id: `track-${index}` }));
    const next = vi.fn()
      .mockResolvedValueOnce({ items: tracks.slice(100, 200), hasMore: true, continuation: 'c3' })
      .mockResolvedValueOnce({ items: tracks.slice(200, 300), hasMore: true, continuation: 'c4' })
      .mockResolvedValueOnce({ items: tracks.slice(300, 400), hasMore: true, continuation: 'c5' })
      .mockResolvedValueOnce({ items: tracks.slice(400, 500), hasMore: true, continuation: 'c6' })
      .mockResolvedValueOnce({ items: tracks.slice(500, 600), hasMore: true, continuation: 'c7' })
      .mockResolvedValueOnce({ items: tracks.slice(600), hasMore: false, continuation: null, expectedCount: 629 });
    const onStop = vi.fn();
    const loaded = await loadAllPages({ items: tracks.slice(0, 100), hasMore: true, continuation: 'c2', expectedCount: 629 }, next, (track) => track.id, { onStop });
    expect(loaded).toHaveLength(629);
    expect(next).toHaveBeenCalledTimes(6);
    expect(onStop).toHaveBeenLastCalledWith({ reason: 'provider-terminal', pagesLoaded: 6 });
  });

  it('does not stop at an arbitrary continuation-page limit', async () => {
    let cursor = 0;
    const next = vi.fn(async () => {
      cursor += 1;
      return { items: [{ id: `track-${cursor}` }], hasMore: cursor < 1001, continuation: cursor < 1001 ? `page-${cursor + 1}` : null };
    });

    const result = await loadAllPages({ items: [{ id: 'track-0' }], hasMore: true, continuation: 'page-1' }, next, (track) => track.id);

    expect(result).toHaveLength(1002);
    expect(next).toHaveBeenCalledTimes(1001);
  });

  it('merges a continuation requested while the initial 100-track page is loading', async () => {
    const initialTracks = Array.from({ length: 100 }, (_, index) => ({ id: `track-${index + 1}` }));
    const continuationTracks = Array.from({ length: 100 }, (_, index) => ({ id: `track-${index + 101}` }));
    let initialLoading = true;
    const next = vi.fn().mockResolvedValue({ items: continuationTracks, hasMore: false, continuation: null });

    const result = await loadAllPages(
      { items: initialTracks, hasMore: true, continuation: 'page-2' },
      async () => {
        expect(initialLoading).toBe(true);
        return next();
      },
      (track) => track.id,
      { onStop: () => { initialLoading = false; } },
    );

    expect(result).toHaveLength(200);
    expect(initialLoading).toBe(false);
    expect(next).toHaveBeenCalledTimes(1);
  });

  it('retains pages already appended when a later continuation fails', async () => {
    const next = vi.fn()
      .mockResolvedValueOnce({ items: [{ id: '2' }], hasMore: true, continuation: 'page-3' })
      .mockRejectedValueOnce(new Error('continuation failed'));
    const appended: { id: string }[] = [];

    await expect(loadAllPages({ items: [{ id: '1' }], hasMore: true }, next, (item) => item.id, {
      onPage: (items) => { appended.push(...items); },
    })).rejects.toThrow('continuation failed');
    expect(appended).toEqual([{ id: '2' }]);
  });

  it('stops a duplicate continuation page without losing the initial tracks', async () => {
    const next = vi.fn().mockResolvedValue({ items: [{ id: '1' }], hasMore: true });
    const onStop = vi.fn();

    await expect(loadAllPages({ items: [{ id: '1' }], hasMore: true }, next, (item) => item.id, { onStop })).resolves.toEqual([{ id: '1' }]);
    expect(onStop).toHaveBeenLastCalledWith({ reason: 'repeated-page', pagesLoaded: 1 });
  });

  it('stops when YouTube repeats a continuation cursor', async () => {
    const next = vi.fn()
      .mockResolvedValueOnce({ items: [{ id: '2' }], hasMore: true, continuation: 'page-2' })
      .mockResolvedValueOnce({ items: [{ id: '3' }], hasMore: true, continuation: 'page-2' });
    const onStop = vi.fn();

    await expect(loadAllPages({ items: [{ id: '1' }], hasMore: true, continuation: 'page-1' }, next, (item) => item.id, { onStop })).resolves.toHaveLength(3);
    expect(next).toHaveBeenCalledTimes(2);
    expect(onStop).toHaveBeenLastCalledWith({ reason: 'repeated-continuation', pagesLoaded: 2 });
  });

  it('does not report a missing next cursor as provider terminal', async () => {
    const onStop = vi.fn();
    const result = await loadAllPages({ items: [{ id: '1' }], hasMore: true, continuation: 'c1' },
      vi.fn().mockResolvedValue({ items: [{ id: '2' }], hasMore: true, continuation: null }),
      (track) => track.id, { onStop });
    expect(result).toEqual([{ id: '1' }, { id: '2' }]);
    expect(onStop).toHaveBeenLastCalledWith({ reason: 'incomplete-chain', pagesLoaded: 1 });
  });

  it('reports a provider terminal that falls short of the advertised count as incomplete', async () => {
    const onStop = vi.fn();
    await loadAllPages({ items: [{ id: '1' }, { id: '2' }], hasMore: false, expectedCount: 3 }, vi.fn(), (track) => track.id, { onStop });
    expect(onStop).toHaveBeenLastCalledWith({ reason: 'expected-count-mismatch', pagesLoaded: 0 });
  });

  it('rejects a response from a stale playlist generation', () => {
    expect(isCurrentPlaylistRequest(1, 2, 'playlist-a', 'playlist-b')).toBe(false);
    expect(isCurrentPlaylistRequest(2, 2, 'playlist-b', 'playlist-b')).toBe(true);
  });
});

describe('playlist pagination operation ownership', () => {
  it('joins simultaneous open and scroll requests to one owner and merges 100 into 200 before terminalizing', async () => {
    const registry = new PlaylistOperationRegistry<{ tracks: { id: string }[]; terminal: boolean }>();
    const initialTracks = Array.from({ length: 100 }, (_, index) => ({ id: `track-${index}` }));
    const continuationTracks = Array.from({ length: 100 }, (_, index) => ({ id: `track-${index + 100}` }));
    let releaseInitial!: (page: { items: { id: string }[]; hasMore: boolean; continuation: string }) => void;
    const initial = new Promise<{ items: { id: string }[]; hasMore: boolean; continuation: string }>((resolve) => { releaseInitial = resolve; });
    const fetchNext = vi.fn().mockResolvedValue({ items: continuationTracks, hasMore: false, continuation: null });
    let ownerCount = 0;

    const open = registry.run('playlist', 7, async () => {
      ownerCount += 1;
      const tracks = await loadAllPages(await initial, fetchNext, (track) => track.id);
      return { tracks, terminal: true };
    });
    const scroll = registry.run('playlist', 7, async () => {
      ownerCount += 1;
      return { tracks: [], terminal: false };
    });
    expect(registry.join('playlist', 7)).toBe(open);
    releaseInitial({ items: initialTracks, hasMore: true, continuation: 'page-2' });

    await expect(Promise.all([open, scroll])).resolves.toEqual([
      { tracks: [...initialTracks, ...continuationTracks], terminal: true },
      { tracks: [...initialTracks, ...continuationTracks], terminal: true },
    ]);
    expect(ownerCount).toBe(1);
    expect(fetchNext).toHaveBeenCalledTimes(1);
    expect(registry.join('playlist', 7)).toBeUndefined();
  });

  it('releases a failed owner so retry can acquire it and settle loading', async () => {
    const registry = new PlaylistOperationRegistry<void>();
    let loading = true;
    const first = registry.run('playlist', 2, async () => {
      try { throw new Error('page fetch failed'); }
      finally { loading = false; }
    });
    await expect(first).rejects.toThrow('page fetch failed');
    expect(loading).toBe(false);
    await expect(registry.run('playlist', 2, async () => undefined)).resolves.toBeUndefined();
  });

  it('keeps playlist switching isolated from an old generation response', async () => {
    const registry = new PlaylistOperationRegistry<string>();
    let activeId = 'playlist-a';
    let currentGeneration = 1;
    let applied: string[] = [];
    let release!: (value: string) => void;
    const oldResponse = new Promise<string>((resolve) => { release = resolve; });
    const oldOperation = registry.run('playlist-a', 1, async () => {
      const value = await oldResponse;
      if (isCurrentPlaylistRequest(1, currentGeneration, 'playlist-a', activeId)) applied.push(value);
      return value;
    });

    activeId = 'playlist-b';
    currentGeneration = 2;
    release('stale page');
    await oldOperation;
    expect(applied).toEqual([]);
  });
});

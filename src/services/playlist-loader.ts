export type PaginationStopReason =
  | 'provider-terminal'
  | 'empty-page'
  | 'duplicate-page'
  | 'repeated-page'
  | 'repeated-continuation'
  | 'invalid-continuation'
  | 'incomplete-chain'
  | 'expected-count-mismatch';

export interface PaginationStop {
  reason: PaginationStopReason;
  pagesLoaded: number;
}

export interface LoadAllPagesOptions<T = unknown> {
  onPage?: (items: T[], page: PaginationPage<T>) => void | Promise<void>;
  onStop?: (stop: PaginationStop) => void | Promise<void>;
}

export interface PaginationPage<T> {
  items: T[];
  hasMore: boolean;
  continuation?: string | null;
  expectedCount?: number | null;
}

export type PlaylistLoadVisibility = 'loading' | 'complete' | 'error' | 'cancelled' | 'stale';

export function shouldRevealPlaylistTracks(state: PlaylistLoadVisibility): boolean {
  return state !== 'loading';
}

export function isCurrentPlaylistRequest(
  requestGeneration: number,
  currentGeneration: number,
  requestPlaylistId: string,
  activePlaylistId: string,
): boolean {
  return requestGeneration === currentGeneration && requestPlaylistId === activePlaylistId;
}

export class PlaylistOperationRegistry<T> {
  private readonly operations = new Map<string, { generation: number; promise: Promise<T> }>();

  run(key: string, generation: number, operation: () => Promise<T>): Promise<T> {
    const current = this.operations.get(key);
    if (current?.generation === generation) return current.promise;

    const record = { generation, promise: Promise.resolve().then(operation) };
    this.operations.set(key, record);
    void record.promise.finally(() => {
      if (this.operations.get(key) === record) this.operations.delete(key);
    }).catch(() => undefined);
    return record.promise;
  }

  join(key: string, generation: number): Promise<T> | undefined {
    const current = this.operations.get(key);
    return current?.generation === generation ? current.promise : undefined;
  }
}

export async function loadAllPages<T>(
  firstPage: T[] | PaginationPage<T>,
  loadNextPage: () => Promise<T[] | PaginationPage<T>>,
  key: (item: T) => string,
  options: LoadAllPagesOptions<T> = {},
): Promise<T[]> {
  const initialItems = Array.isArray(firstPage) ? firstPage : firstPage.items;
  const result = [...initialItems];
  const seen = new Set(result.map(key));
  const seenPages = new Set([JSON.stringify(initialItems.map(key))]);
  const seenContinuations = new Set<string>();
  if (!Array.isArray(firstPage) && firstPage.continuation?.trim()) seenContinuations.add(firstPage.continuation);
  let pagesLoaded = 0;

  const stop = async (reason: PaginationStopReason) => {
    await options.onStop?.({ reason, pagesLoaded });
    return result;
  };

  if (!Array.isArray(firstPage) && !firstPage.hasMore) {
    return Number.isFinite(firstPage.expectedCount) && result.length < Number(firstPage.expectedCount)
      ? stop('expected-count-mismatch')
      : stop('provider-terminal');
  }

  while (true) {
    const response = await loadNextPage();
    pagesLoaded += 1;
    const page = Array.isArray(response) ? response : response.items;
    if (!page.length) return stop('empty-page');

    const pageSignature = JSON.stringify(page.map(key));
    if (seenPages.has(pageSignature)) return stop('repeated-page');
    seenPages.add(pageSignature);

    const fresh = page.filter((item) => !seen.has(key(item)));
    if (!fresh.length) return stop('duplicate-page');

    fresh.forEach((item) => seen.add(key(item)));
    result.push(...fresh);
    await options.onPage?.(fresh, Array.isArray(response) ? { items: page, hasMore: true } : response);
    if (!Array.isArray(response)) {
      if (!response.hasMore) {
        const expectedCount = response.expectedCount ?? (!Array.isArray(firstPage) ? firstPage.expectedCount : undefined);
        return Number.isFinite(expectedCount) && result.length < Number(expectedCount)
          ? stop('expected-count-mismatch')
          : stop('provider-terminal');
      }
      const continuation = response.continuation?.trim();
      if (!continuation) return stop('incomplete-chain');
      if (seenContinuations.has(continuation)) return stop('repeated-continuation');
      seenContinuations.add(continuation);
    }
  }
}

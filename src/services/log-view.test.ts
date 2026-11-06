import { describe, expect, it } from 'vitest';
import type { AppLogEntry } from './app-logs';
import { countLogs, filterLogs, formatLogDetails, isNearBottom, logEntryToText, sourceHue } from './log-view';

const entry = (id: number, patch: Partial<AppLogEntry> = {}): AppLogEntry => ({
  id, timestamp: 1_700_000_000 + id, level: 'info', source: 'audio-engine', operation: 'audio', message: `message ${id}`, ...patch,
});

const logs = [
  entry(1),
  entry(2, { level: 'error', source: 'youtube-stream', operation: 'stream', message: 'googlevideo failed', code: 'E_HTTP_403' }),
  entry(3, { source: 'youtube-playlist', operation: 'playlist', window: 'main', message: 'playlist loaded' }),
  entry(4, { level: 'error', source: 'audio-engine', message: 'decoder error' }),
  entry(5, { level: 'warn', source: 'audio-engine', message: 'retry scheduled' }),
  entry(6, { level: 'debug', source: 'audio-engine', message: 'snapshot' }),
];

const noSources = new Set<string>();

describe('filterLogs', () => {
  it('keeps arrival order (oldest first)', () => {
    expect(filterLogs(logs, { minLevel: 'debug', sources: noSources, search: '' }).map((item) => item.id)).toEqual([1, 2, 3, 4, 5, 6]);
  });

  it('treats the level as a minimum severity', () => {
    const ids = (minLevel: 'debug' | 'info' | 'warn' | 'error') => filterLogs(logs, { minLevel, sources: noSources, search: '' }).map((item) => item.id);
    expect(ids('info')).toEqual([1, 2, 3, 4, 5]);
    expect(ids('warn')).toEqual([2, 4, 5]);
    expect(ids('error')).toEqual([2, 4]);
  });

  it('filters by level, by any of the selected sources and by text across fields', () => {
    expect(filterLogs(logs, { minLevel: 'error', sources: noSources, search: '' }).map((item) => item.id)).toEqual([2, 4]);
    expect(filterLogs(logs, { minLevel: 'debug', sources: new Set(['audio-engine', 'youtube-playlist']), search: '' }).map((item) => item.id)).toEqual([1, 3, 4, 5, 6]);
    expect(filterLogs(logs, { minLevel: 'debug', sources: noSources, search: '  E_HTTP_403 ' }).map((item) => item.id)).toEqual([2]);
    expect(filterLogs(logs, { minLevel: 'debug', sources: noSources, search: 'MAIN' }).map((item) => item.id)).toEqual([3]);
  });

  it('combines every filter', () => {
    expect(filterLogs(logs, { minLevel: 'error', sources: new Set(['audio-engine']), search: 'decoder' }).map((item) => item.id)).toEqual([4]);
    expect(filterLogs(logs, { minLevel: 'error', sources: new Set(['audio-engine']), search: 'retry' })).toEqual([]);
  });
});

describe('countLogs', () => {
  it('counts totals, errors and sources, busiest source first', () => {
    const counts = countLogs(logs);
    expect(counts.total).toBe(6);
    expect(counts.byLevel).toEqual({ debug: 1, info: 2, warn: 1, error: 2 });
    expect(counts.error).toBe(2);
    expect(counts.bySource[0]).toEqual({ source: 'audio-engine', count: 4, errors: 1 });
    expect(counts.bySource.map((item) => item.source)).toEqual(['audio-engine', 'youtube-playlist', 'youtube-stream']);
  });

  it('handles an empty stream', () => {
    expect(countLogs([])).toEqual({ total: 0, error: 0, byLevel: { debug: 0, info: 0, warn: 0, error: 0 }, bySource: [] });
  });
});

describe('details and text', () => {
  it('formats details only when present', () => {
    expect(formatLogDetails(entry(1))).toBeNull();
    expect(formatLogDetails(entry(1, { details: {} }))).toBeNull();
    expect(formatLogDetails(entry(1, { details: { bytes: 12 } }))).toBe('{\n  "bytes": 12\n}');
  });

  it('copies every field of an entry', () => {
    const text = logEntryToText(entry(2, { level: 'error', window: 'main', code: 'E1', details: { a: 1 } }));
    expect(text).toContain('ERROR | audio-engine | audio | main | E1');
    expect(text).toContain('message 2');
    expect(text).toContain('"a": 1');
  });
});

describe('sourceHue', () => {
  it('is stable and never lands on the red/amber range reserved for warn and error', () => {
    expect(sourceHue('audio-engine')).toBe(sourceHue('audio-engine'));
    for (const name of ['audio-engine', 'youtube-stream', 'native', 'youtube-auth', 'a', 'zzzz', '']) {
      const hue = sourceHue(name);
      expect(hue).toBeGreaterThanOrEqual(95);
      expect(hue).toBeLessThan(310);
    }
  });

  it('separates the usual sources', () => {
    const hues = new Set(['audio-engine', 'youtube-stream', 'youtube-playlist', 'youtube-auth', 'native', 'youtube-session'].map(sourceHue));
    expect(hues.size).toBeGreaterThanOrEqual(5);
  });
});

describe('isNearBottom', () => {
  it('follows the tail only close to the bottom', () => {
    expect(isNearBottom(952, 500, 1500)).toBe(true);
    expect(isNearBottom(900, 500, 1500)).toBe(false);
    expect(isNearBottom(0, 500, 400)).toBe(true);
  });
});

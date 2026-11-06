import type { AppLogEntry, LogLevel } from './app-logs';

export const LOG_LEVELS: readonly LogLevel[] = ['debug', 'info', 'warn', 'error'];

/** Severity rank: a filter at `warn` shows warn and error. Unknown levels rank as info. */
export function levelRank(level: string): number {
  const rank = LOG_LEVELS.indexOf(level as LogLevel);
  return rank === -1 ? 1 : rank;
}

/** Hues kept for sources: green through violet. Red and amber stay reserved for errors and warnings. */
const SOURCE_HUE_MIN = 95;
const SOURCE_HUE_SPAN = 215;

/** Stable hue (degrees) for a source name, always outside the warn/error colour range. */
export function sourceHue(source: string): number {
  let hash = 2166136261;
  for (let i = 0; i < source.length; i += 1) {
    hash ^= source.charCodeAt(i);
    hash = Math.imul(hash, 16777619) >>> 0;
  }
  return SOURCE_HUE_MIN + (hash % SOURCE_HUE_SPAN);
}

export interface LogFilter {
  /** Minimum severity to show. */
  minLevel: LogLevel;
  /** Empty set means every source. */
  sources: ReadonlySet<string>;
  search: string;
}

export interface LogCounts {
  total: number;
  error: number;
  byLevel: Record<LogLevel, number>;
  bySource: { source: string; count: number; errors: number }[];
}

/** Entries keep arrival order (oldest first): a log stream reads top to bottom, like a terminal. */
export function filterLogs(entries: readonly AppLogEntry[], filter: LogFilter): AppLogEntry[] {
  const search = filter.search.trim().toLowerCase();
  return entries.filter((entry) => {
    if (levelRank(entry.level) < levelRank(filter.minLevel)) return false;
    if (filter.sources.size > 0 && !filter.sources.has(entry.source)) return false;
    if (!search) return true;
    const haystack = `${entry.source} ${entry.window ?? ''} ${entry.operation} ${entry.code ?? ''} ${entry.message}`;
    return haystack.toLowerCase().includes(search);
  });
}

export function countLogs(entries: readonly AppLogEntry[]): LogCounts {
  const sources = new Map<string, { count: number; errors: number }>();
  let error = 0;
  const byLevel: Record<LogLevel, number> = { debug: 0, info: 0, warn: 0, error: 0 };
  for (const entry of entries) {
    byLevel[LOG_LEVELS[levelRank(entry.level)]] += 1;
    const bucket = sources.get(entry.source) ?? { count: 0, errors: 0 };
    bucket.count += 1;
    if (entry.level === 'error') {
      bucket.errors += 1;
      error += 1;
    }
    sources.set(entry.source, bucket);
  }
  return {
    total: entries.length,
    error,
    byLevel,
    bySource: Array.from(sources, ([source, value]) => ({ source, ...value })).sort((a, b) => b.count - a.count || a.source.localeCompare(b.source)),
  };
}

export function formatLogTime(timestamp: number): string {
  return new Date(timestamp * 1000).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

/** Pretty JSON for the structured details, or null when there is nothing to show. */
export function formatLogDetails(entry: AppLogEntry): string | null {
  if (!entry.details || Object.keys(entry.details).length === 0) return null;
  try {
    return JSON.stringify(entry.details, null, 2);
  } catch {
    return null;
  }
}

/** One plain-text line per entry, used by "copy" so a pasted log keeps every field. */
export function logEntryToText(entry: AppLogEntry): string {
  const parts = [formatLogTime(entry.timestamp), entry.level.toUpperCase(), entry.source, entry.operation];
  if (entry.window) parts.push(entry.window);
  if (entry.code) parts.push(entry.code);
  const header = parts.join(' | ');
  const details = formatLogDetails(entry);
  return details ? `${header}\n${entry.message}\n${details}` : `${header}\n${entry.message}`;
}

/** The view follows the tail only while the reader is within `threshold` pixels of the bottom. */
export function isNearBottom(scrollTop: number, clientHeight: number, scrollHeight: number, threshold = 48): boolean {
  return scrollHeight - (scrollTop + clientHeight) <= threshold;
}

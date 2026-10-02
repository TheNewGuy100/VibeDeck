export interface AnalyzerSnapshot {
  levels: number[];
  sampleRate?: number;
  revision?: number;
  timestamp?: number;
}

/** Durable preference for the general visualizer-controls disclosure. */
export interface VisualizerControlsPreference {
  open: boolean;
  revision: number;
}

export interface SetVisualizerControlsPreferenceRequest {
  open: boolean;
  revision: number;
}

export interface SerpentsPreference {
  version: 1;
  revision: number;
  snakes: import('../visualizers/serpents-layout').SerpentSnake[];
  lights: import('../visualizers/serpents-layout').SerpentLight[];
}

export type SetSerpentsPreferenceRequest = Omit<SerpentsPreference, 'version'>;

export function parseSerpentsPreference(value: unknown): SerpentsPreference {
  if (!value || typeof value !== 'object') throw new Error('Preferências do Serpents inválidas.');
  const preference = value as Partial<SerpentsPreference>;
  if (preference.version !== 1 || !Number.isSafeInteger(preference.revision) || (preference.revision ?? -1) < 0
    || !Array.isArray(preference.snakes) || !Array.isArray(preference.lights)) throw new Error('Preferências do Serpents inválidas.');
  return { version: 1, revision: preference.revision!, snakes: preference.snakes, lights: preference.lights };
}

export function parseVisualizerControlsPreference(value: unknown): VisualizerControlsPreference {
  if (!value || typeof value !== 'object') throw new Error('Preferência dos controles do visualizador inválida.');
  const preference = value as Partial<VisualizerControlsPreference>;
  if (typeof preference.open !== 'boolean' || !Number.isSafeInteger(preference.revision) || (preference.revision ?? -1) < 0) {
    throw new Error('Preferência dos controles do visualizador inválida.');
  }
  return { open: preference.open, revision: preference.revision! };
}

export type AnalyzerEventKind = 'levelsUpdated' | 'reset' | 'stopped';

export interface AnalyzerEvent {
  kind: AnalyzerEventKind;
  snapshot: AnalyzerSnapshot;
}

export function parseAnalyzerSnapshot(value: unknown): AnalyzerSnapshot {
  if (Array.isArray(value)) return { levels: value as number[] };
  if (!value || typeof value !== 'object' || !Array.isArray((value as AnalyzerSnapshot).levels)) {
    throw new Error('O bridge retornou um snapshot do analyzer inválido.');
  }
  return value as AnalyzerSnapshot;
}

export function isAnalyzerEvent(value: unknown): value is AnalyzerEvent {
  if (!value || typeof value !== 'object') return false;
  const event = value as Partial<AnalyzerEvent>;
  return (event.kind === 'levelsUpdated' || event.kind === 'reset' || event.kind === 'stopped')
    && Boolean(event.snapshot && Array.isArray(event.snapshot.levels));
}

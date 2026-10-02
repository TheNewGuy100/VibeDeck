import { invoke } from '@tauri-apps/api/core';
import { ref, watch } from 'vue';
import { parseSerpentsPreference, type SerpentsPreference, type SetSerpentsPreferenceRequest } from '../interfaces/visualizer';
import { defaultSerpentLights, defaultSnakeLayout, normalizeSerpentLights, normalizeSnakeLayout, type SerpentLight, type SerpentSnake } from '../visualizers/serpents-layout';

export interface SerpentsPreferenceBridge { get(): Promise<unknown>; set(value: SetSerpentsPreferenceRequest): Promise<unknown> }
export const serpentsSnakes = ref(defaultSnakeLayout());
export const serpentsLights = ref(defaultSerpentLights());
export function isNativeSerpentsPreferencesAvailable() {
  return typeof (globalThis as typeof globalThis & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined';
}
const bridge: SerpentsPreferenceBridge = {
  get: () => invoke('get_serpents_preference'),
  set: value => invoke('set_serpents_preference', value),
};

export function createSerpentsPreferenceController(
  nativeBridge: SerpentsPreferenceBridge = bridge,
  available: () => boolean = isNativeSerpentsPreferencesAvailable,
) {
  let revision = 0;
  let intent = 0;
  const defaults = (): SerpentsPreference => ({ version: 1, revision: 0, snakes: defaultSnakeLayout(), lights: defaultSerpentLights() });
  return {
    async load(apply: (value: { snakes: SerpentSnake[]; lights: SerpentLight[] }) => void) {
      const current = intent;
      if (!available()) return;
      try {
        const saved = parseSerpentsPreference(await nativeBridge.get());
        if (intent !== current) return;
        revision = Math.max(revision, saved.revision);
        apply({ snakes: normalizeSnakeLayout(saved.snakes), lights: normalizeSerpentLights(saved.lights) });
      } catch { /* Native load failures retain volatile defaults/current edits. */ }
    },
    async save(snakes: SerpentSnake[], lights: SerpentLight[]) {
      const requestIntent = ++intent;
      if (!available()) return;
      const request = { revision: ++revision, snakes: normalizeSnakeLayout(snakes), lights: normalizeSerpentLights(lights) };
      try {
        const saved = parseSerpentsPreference(await nativeBridge.set(request));
        if (requestIntent !== intent) return;
        revision = Math.max(revision, saved.revision);
      } catch { /* WebView remains usable; browser/native failure fallback is memory-only. */ }
    },
  };
}

let sharedController: ReturnType<typeof createSerpentsPreferenceController> | undefined;
let loadPromise: Promise<void> | undefined;
function getSharedController() {
  sharedController ??= createSerpentsPreferenceController();
  return sharedController;
}

export function loadSharedSerpentsPreferences() {
  loadPromise ??= getSharedController().load(value => {
    serpentsSnakes.value = value.snakes;
    serpentsLights.value = value.lights;
  });
  return loadPromise;
}

export function updateSerpentsPreferences(snakes: SerpentSnake[], lights: SerpentLight[]) {
  serpentsSnakes.value = normalizeSnakeLayout(snakes);
  serpentsLights.value = normalizeSerpentLights(lights);
  void getSharedController().save(serpentsSnakes.value, serpentsLights.value);
}

watch([serpentsSnakes, serpentsLights], ([snakes, lights]) => {
  void getSharedController().save(snakes, lights);
}, { deep: true });

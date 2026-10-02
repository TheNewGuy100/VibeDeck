import { invoke } from '@tauri-apps/api/core';
import {
  parseVisualizerControlsPreference,
  type SetVisualizerControlsPreferenceRequest,
  type VisualizerControlsPreference,
} from '../interfaces/visualizer';

export const DEFAULT_VISUALIZER_CONTROLS_OPEN = true;

export interface VisualizerPreferenceBridge {
  get(): Promise<unknown>;
  set(request: SetVisualizerControlsPreferenceRequest): Promise<unknown>;
}

export function isNativeVisualizerPreferenceAvailable() {
  return typeof (globalThis as typeof globalThis & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined';
}

const nativeBridge: VisualizerPreferenceBridge = {
  get: () => invoke('get_visualizer_controls_preference'),
  set: ({ open, revision }) => invoke('set_visualizer_controls_preference', { open, revision }),
};

/** Per-component synchronization state: native revision is the durable source of truth. */
export function createVisualizerPreferenceController(
  bridge: VisualizerPreferenceBridge = nativeBridge,
  available: () => boolean = isNativeVisualizerPreferenceAvailable,
) {
  let revision = 0;
  let intent = 0;
  let lastPersisted: boolean | undefined;

  return {
    async load(onValue: (open: boolean) => void) {
      const requestIntent = intent;
      if (!available()) {
        onValue(DEFAULT_VISUALIZER_CONTROLS_OPEN);
        return;
      }
      try {
        const stored: VisualizerControlsPreference = parseVisualizerControlsPreference(await bridge.get());
        if (requestIntent !== intent) return;
        revision = Math.max(revision, stored.revision);
        lastPersisted = stored.open;
        onValue(stored.open);
      } catch {
        // Native unavailable/failed: retain the current default or user intent in volatile memory.
      }
    },
    async set(open: boolean) {
      const currentIntent = ++intent;
      if (!available() || lastPersisted === open) return;
      const requestRevision = ++revision;
      try {
        const saved = parseVisualizerControlsPreference(await bridge.set({ open, revision: requestRevision }));
        if (currentIntent !== intent) return;
        revision = Math.max(revision, saved.revision);
        lastPersisted = saved.open;
      } catch {
        // Keep the interactive choice in memory; a future toggle can retry native persistence.
      }
    },
  };
}

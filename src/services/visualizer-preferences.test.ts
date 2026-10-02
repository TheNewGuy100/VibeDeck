import { describe, expect, it, vi } from 'vitest';
import {
  createVisualizerPreferenceController,
  DEFAULT_VISUALIZER_CONTROLS_OPEN,
  type VisualizerPreferenceBridge,
} from './visualizer-preferences';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe('general visualizer controls preference', () => {
  it('keeps the existing open default when native persistence is unavailable', async () => {
    const bridge = { get: vi.fn(), set: vi.fn() } as unknown as VisualizerPreferenceBridge;
    const controller = createVisualizerPreferenceController(bridge, () => false);
    let open = !DEFAULT_VISUALIZER_CONTROLS_OPEN;
    await controller.load((value) => { open = value; });
    await controller.set(false);
    expect(open).toBe(true);
    expect(bridge.get).not.toHaveBeenCalled();
    expect(bridge.set).not.toHaveBeenCalled();
  });

  it('persists open and close toggles with increasing revisions', async () => {
    const requests: Array<{ open: boolean; revision: number }> = [];
    const bridge: VisualizerPreferenceBridge = {
      get: async () => ({ open: true, revision: 0 }),
      set: async (request) => { requests.push(request); return request; },
    };
    const controller = createVisualizerPreferenceController(bridge, () => true);
    await controller.set(false);
    await controller.set(true);
    expect(requests).toEqual([{ open: false, revision: 1 }, { open: true, revision: 2 }]);
  });

  it('rehydrates the saved value after recreation', async () => {
    let persisted = { open: false, revision: 1 };
    const bridge: VisualizerPreferenceBridge = {
      get: async () => persisted,
      set: async (request) => (persisted = request),
    };
    const component = createVisualizerPreferenceController(bridge, () => true);
    const refresh = createVisualizerPreferenceController(bridge, () => true);
    await component.set(false);
    let hydrated = true;
    await refresh.load((value) => { hydrated = value; });
    expect(hydrated).toBe(false);
  });

  it('keeps the latest choice during rapid toggles and ignores late saves', async () => {
    const first = deferred<unknown>();
    const second = deferred<unknown>();
    const set = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const controller = createVisualizerPreferenceController({ get: async () => ({ open: true, revision: 0 }), set }, () => true);
    const olderSave = controller.set(false);
    const newerSave = controller.set(true);
    second.resolve({ open: true, revision: 2 });
    await newerSave;
    first.resolve({ open: false, revision: 1 });
    await olderSave;
    expect(set).toHaveBeenNthCalledWith(1, { open: false, revision: 1 });
    expect(set).toHaveBeenNthCalledWith(2, { open: true, revision: 2 });
  });

  it('does not let a delayed hydration overwrite a user toggle', async () => {
    const response = deferred<unknown>();
    const set = vi.fn(async (request) => request);
    const controller = createVisualizerPreferenceController({ get: () => response.promise, set }, () => true);
    const applied: boolean[] = [];
    const loading = controller.load((value) => applied.push(value));
    await controller.set(false);
    response.resolve({ open: true, revision: 0 });
    await loading;
    expect(applied).toEqual([]);
  });
});

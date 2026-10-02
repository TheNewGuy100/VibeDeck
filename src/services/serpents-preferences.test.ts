import { describe, expect, it, vi } from 'vitest';
import { createSerpentsPreferenceController, type SerpentsPreferenceBridge } from './serpents-preferences';
import { defaultSerpentLights, defaultSnakeLayout } from '../visualizers/serpents-layout';

const response = (revision: number) => ({ version: 1 as const, revision, snakes: defaultSnakeLayout(), lights: defaultSerpentLights() });

describe('Serpents native preference flow', () => {
  it('hydrates native snapshot, and keeps browser fallback volatile', async () => {
    const get = vi.fn().mockResolvedValue(response(8));
    const bridge: SerpentsPreferenceBridge = { get, set: vi.fn() };
    const apply = vi.fn();
    await createSerpentsPreferenceController(bridge, () => true).load(apply);
    expect(get).toHaveBeenCalledOnce();
    expect(apply).toHaveBeenCalledWith({ snakes: response(8).snakes, lights: response(8).lights });
    await createSerpentsPreferenceController(bridge, () => false).load(apply);
    expect(get).toHaveBeenCalledOnce();
  });
  it('writes increasing revisions and ignores stale load completion after user save', async () => {
    let resolveLoad!: (value: unknown) => void;
    const get = vi.fn(() => new Promise(resolve => { resolveLoad = resolve; }));
    const set = vi.fn().mockImplementation(async request => ({ version: 1, ...request }));
    const controller = createSerpentsPreferenceController({ get, set }, () => true);
    const apply = vi.fn();
    const loading = controller.load(apply);
    await controller.save(defaultSnakeLayout(), defaultSerpentLights());
    resolveLoad(response(0));
    await loading;
    expect(set.mock.calls[0][0].revision).toBe(1);
    expect(apply).not.toHaveBeenCalled();
  });
  it('rejects malformed or unsupported preference versions', async () => {
    const controller = createSerpentsPreferenceController({ get: async () => ({ version: 2 }), set: vi.fn() }, () => true);
    const apply = vi.fn();
    await controller.load(apply);
    expect(apply).not.toHaveBeenCalled();
  });
});

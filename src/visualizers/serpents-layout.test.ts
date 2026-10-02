import { describe, expect, it } from 'vitest';
import { addLight, addSnake, defaultSerpentLights, defaultSnakeLayout, LIGHT_LIMITS, MAX_LIGHTS, MAX_SNAKES, MIN_LIGHTS, MIN_SNAKES, normalizeSerpentLights, normalizeSnakeLayout, removeLight, removeSnake, serpentDepthPhase, serpentDirection, serpentRole } from './serpents-layout';

describe('Serpents CPU layout', () => {
  it('has stable defaults, side-ascending and center-descending lanes with alternating weave', () => {
    const layout = defaultSnakeLayout();
    expect(layout.map(item => item.id)).toEqual(['serpent-1', 'serpent-2', 'serpent-3', 'serpent-4', 'serpent-5', 'serpent-6']);
    expect(layout.slice(0, 2).map(item => serpentDirection(item.lane))).toEqual([1, 1]);
    expect(layout.slice(2).every(item => serpentRole(item.lane) === 'center-descending')).toBe(true);
    expect(serpentDepthPhase(0, 1)).toBeGreaterThan(0);
    expect(serpentDepthPhase(1, 1)).toBeLessThan(0);
  });
  it('bounds counts, clamps persisted positions and protects minimums', () => {
    expect(normalizeSnakeLayout([])).toHaveLength(MIN_SNAKES);
    expect(normalizeSnakeLayout(Array.from({ length: 20 }, () => ({ x: 100, z: -100, scale: 9, phase: 99 })))).toHaveLength(MAX_SNAKES);
    expect(normalizeSerpentLights([{ id: 'x', x: 100, y: -100, z: 99 }])[0]).toMatchObject({ x: 12, y: -9, z: 14 });
    expect(normalizeSerpentLights([])).toHaveLength(MIN_LIGHTS);
    expect(LIGHT_LIMITS.x).toEqual([-12, 12]);
  });
  it('adds/removes within snake and light count bounds', () => {
    const snakes = defaultSnakeLayout();
    expect(addSnake(snakes)).toHaveLength(snakes.length + 1);
    expect(removeSnake(snakes, snakes[0].id)).toHaveLength(snakes.length - 1);
    expect(removeSnake(snakes.slice(0, MIN_SNAKES), 'serpent-1')).toHaveLength(MIN_SNAKES);
    const lights = defaultSerpentLights();
    expect(addLight(lights)).toHaveLength(lights.length + 1);
    expect(removeLight(lights, lights[0].id)).toHaveLength(lights.length - 1);
    expect(addSnake(Array.from({ length: MAX_SNAKES }, (_, i) => ({ ...snakes[0], id: String(i) })))).toHaveLength(MAX_SNAKES);
    expect(addLight(Array.from({ length: MAX_LIGHTS }, (_, i) => ({ ...lights[0], id: String(i) })))).toHaveLength(MAX_LIGHTS);
  });
  it('keeps stable light IDs and default locations', () => {
    expect(defaultSerpentLights().map(light => light.id)).toEqual(['key-front', 'key-fill', 'key-left', 'key-right']);
  });
});

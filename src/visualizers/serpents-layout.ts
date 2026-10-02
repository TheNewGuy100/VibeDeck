export type SerpentLane = 'left' | 'right' | 'center';
export interface SerpentSnake { id: string; lane: SerpentLane; x: number; z: number; scale: number; phase: number }
export interface SerpentLight { id: string; x: number; y: number; z: number }
export const MIN_SNAKES = 2;
export const MAX_SNAKES = 8;
export const MIN_LIGHTS = 1;
export const MAX_LIGHTS = 8;
export const SNAKE_LIMITS = { x: [-9, 9] as const, z: [-6, 3] as const, scale: [0.5, 1.4] as const, phase: [0, Math.PI * 2] as const };
export const LIGHT_LIMITS = { x: [-12, 12] as const, y: [-9, 9] as const, z: [-10, 14] as const };

export function clamp(value: number, [min, max]: readonly [number, number], fallback: number) {
  return Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
}

export function defaultSnakeLayout(): SerpentSnake[] {
  return Array.from({ length: 6 }, (_, i) => ({
    id: `serpent-${i + 1}`, lane: i === 0 ? 'left' : i === 1 ? 'right' : 'center',
    x: [-7.5, 7.5, -2.4, 0, 2.4, 0][i], z: i % 2 ? -0.8 : 0.5, scale: 1, phase: i * 0.72,
  }));
}

export function defaultSerpentLights(): SerpentLight[] {
  return [
    { id: 'key-front', x: 0, y: 4, z: 13 }, { id: 'key-fill', x: 0, y: -5, z: 10 },
    { id: 'key-left', x: -10, y: 1, z: 7 }, { id: 'key-right', x: 10, y: 1, z: 7 },
  ];
}

export function clampSnake(value: Partial<SerpentSnake>, index: number): SerpentSnake {
  return {
    id: typeof value.id === 'string' && value.id ? value.id : `serpent-${index + 1}`,
    lane: value.lane === 'left' || value.lane === 'right' ? value.lane : 'center',
    x: clamp(Number(value.x), SNAKE_LIMITS.x, 0), z: clamp(Number(value.z), SNAKE_LIMITS.z, 0),
    scale: clamp(Number(value.scale), SNAKE_LIMITS.scale, 1), phase: clamp(Number(value.phase), SNAKE_LIMITS.phase, 0),
  };
}

export function clampLight(value: Partial<SerpentLight>, index: number): SerpentLight {
  return { id: typeof value.id === 'string' && value.id ? value.id : `key-${index + 1}`,
    x: clamp(Number(value.x), LIGHT_LIMITS.x, 0), y: clamp(Number(value.y), LIGHT_LIMITS.y, 0), z: clamp(Number(value.z), LIGHT_LIMITS.z, 0) };
}

export function normalizeSnakeLayout(values: Partial<SerpentSnake>[]) {
  const list = values.slice(0, MAX_SNAKES).map(clampSnake);
  return list.length >= MIN_SNAKES ? list : [...list, ...defaultSnakeLayout().slice(0, MIN_SNAKES - list.length)];
}

export function normalizeSerpentLights(values: Partial<SerpentLight>[]) {
  const list = values.slice(0, MAX_LIGHTS).map(clampLight);
  return list.length ? list : defaultSerpentLights().slice(0, MIN_LIGHTS);
}

export function addSnake(values: SerpentSnake[]): SerpentSnake[] {
  if (values.length >= MAX_SNAKES) return values;
  const index = values.length;
  return [...values, clampSnake({ id: `serpent-${Date.now()}-${index}`, lane: 'center', x: (index % 2 ? -1 : 1) * (1 + (index % 3) * 0.7), z: index % 2 ? -1.2 : 0.8, scale: 1, phase: index * 0.72 }, index)];
}

export function removeSnake(values: SerpentSnake[], id: string): SerpentSnake[] {
  return values.length <= MIN_SNAKES ? values : values.filter(item => item.id !== id);
}

export function addLight(values: SerpentLight[]): SerpentLight[] {
  if (values.length >= MAX_LIGHTS) return values;
  const index = values.length;
  return [...values, clampLight({ id: `key-${Date.now()}-${index}`, x: (index % 2 ? -1 : 1) * 5, y: 0, z: 4 }, index)];
}

export function removeLight(values: SerpentLight[], id: string): SerpentLight[] {
  return values.length <= MIN_LIGHTS ? values : values.filter(item => item.id !== id);
}

export function serpentRole(lane: SerpentLane) { return lane === 'center' ? 'center-descending' : 'side-ascending'; }
export function serpentDirection(lane: SerpentLane): 1 | -1 { return lane === 'center' ? -1 : 1; }
export function serpentDepthPhase(index: number, phase: number) { return (index % 2 ? -1 : 1) * Math.sin(phase); }

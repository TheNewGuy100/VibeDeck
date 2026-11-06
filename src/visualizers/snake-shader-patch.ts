import { ShaderLib, Vector2, Vector3 } from 'three';
import type { BufferGeometry } from 'three';

/**
 * Shader additions for the snake body (MeshStandardMaterial):
 *  - the normal map flows along the body from one phase value, in whole tiles per loop, so the
 *    loop closes exactly when the phase wraps;
 *  - the tube swells radially (before skinning, in rest space) on the bass, plus pulses that
 *    travel along the body.
 *
 * "Along the body" is the contract V: V = 0 at the head, 1 at the tail. glTF stores 1 - V, so
 * the shader works with `1.0 - uv.y`.
 */

export const MAX_PULSES = 4;

export interface SnakeUniforms {
  uPhase: { value: number };
  /** Signed whole tiles the normal map moves per loop: flow direction * sign of the UV slope * tiles. */
  uScroll: { value: number };
  /** Radial growth as a fraction of the local radius (0 = none). */
  uSwell: { value: number };
  /** Unit axis of the tube in rest space. */
  uAxis: { value: Vector3 };
  /** (position along the body, amplitude) for each pulse; amplitude 0 disables the slot. */
  uPulses: { value: Vector2[] };
}

export function createSnakeUniforms(): SnakeUniforms {
  return {
    uPhase: { value: 0 },
    uScroll: { value: 1 },
    uSwell: { value: 0 },
    uAxis: { value: new Vector3(1, 0, 0) },
    uPulses: { value: Array.from({ length: MAX_PULSES }, () => new Vector2(0, 0)) },
  };
}

const UNIFORM_DECLARATIONS = `#include <common>
uniform float uPhase;
uniform float uScroll;
uniform float uSwell;
uniform vec3 uAxis;
uniform vec2 uPulses[${MAX_PULSES}];`;

const SCROLL = `#include <uv_vertex>
#ifdef USE_NORMALMAP
  vNormalMapUv.y -= uScroll * uPhase;
#endif`;

const SWELL = `#include <begin_vertex>
float snakeAlong = 1.0 - uv.y;
vec3 snakeRadial = position - dot(position, uAxis) * uAxis;
float snakeGrow = uSwell;
for (int i = 0; i < ${MAX_PULSES}; i++) {
  float d = snakeAlong - uPulses[i].x;
  snakeGrow += uPulses[i].y * exp(-d * d * 220.0);
}
transformed += snakeRadial * snakeGrow;`;

const replaceOnce = (source: string, needle: string, replacement: string): string => {
  if (source.split(needle).length !== 2) {
    throw new Error(`snake shader patch: expected exactly one "${needle}"`);
  }
  return source.replace(needle, replacement);
};

/** Pure string patch so it can be tested against the real three.js shader without a GPU. */
export function patchSnakeVertexShader(source: string): string {
  let patched = replaceOnce(source, '#include <common>', UNIFORM_DECLARATIONS);
  patched = replaceOnce(patched, '#include <uv_vertex>', SCROLL);
  patched = replaceOnce(patched, '#include <begin_vertex>', SWELL);
  return patched;
}

interface PatchableShader {
  uniforms: Record<string, { value: unknown }>;
  vertexShader: string;
}

/** Wires the uniforms and the patched vertex shader into a MeshStandardMaterial's compile hook. */
export function applySnakeShader(shader: PatchableShader, uniforms: SnakeUniforms): void {
  Object.assign(shader.uniforms, uniforms);
  shader.vertexShader = patchSnakeVertexShader(shader.vertexShader);
}

export const SNAKE_PROGRAM_KEY = 'vibedeck-snake-v2';

/** The standard material's vertex shader as three.js ships it, for tests and tooling. */
export const standardVertexShader = (): string => ShaderLib.standard.vertexShader;

/**
 * Sign of d(normal-map v)/d(contract V), measured on the geometry (glTF UV 0 holds 1 - V, UV 1 the
 * normal-map coordinates). It tells the shader which way to move the sampling so the pattern flows
 * toward the tail, whatever the UV transform baked into the model.
 */
export function normalUvSlopeSign(geometry: BufferGeometry): number {
  const along = geometry.getAttribute('uv');
  const normal = geometry.getAttribute('uv1');
  if (!along || !normal || along.count !== normal.count || along.count < 2) return 0;
  let meanV = 0;
  let meanY = 0;
  for (let i = 0; i < along.count; i += 1) {
    meanV += 1 - along.getY(i);
    meanY += normal.getY(i);
  }
  meanV /= along.count;
  meanY /= along.count;
  let covariance = 0;
  for (let i = 0; i < along.count; i += 1) {
    covariance += (1 - along.getY(i) - meanV) * (normal.getY(i) - meanY);
  }
  return Math.sign(covariance);
}

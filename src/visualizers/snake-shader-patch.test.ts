import { BufferGeometry, Float32BufferAttribute } from 'three';
import { describe, expect, it } from 'vitest';
import {
  MAX_PULSES,
  createSnakeUniforms,
  normalUvSlopeSign,
  patchSnakeVertexShader,
  standardVertexShader,
} from './snake-shader-patch';

describe('patchSnakeVertexShader', () => {
  it('patches the real three.js standard vertex shader exactly once per hook', () => {
    const patched = patchSnakeVertexShader(standardVertexShader());
    expect(patched).toContain('uniform float uPhase;');
    expect(patched).toContain('vNormalMapUv.y -= uScroll * uPhase;');
    expect(patched).toContain('transformed += snakeRadial * snakeGrow;');
    expect(patched).toContain(`uniform vec2 uPulses[${MAX_PULSES}];`);
    expect(patched.split('#include <begin_vertex>').length).toBe(2);
  });

  it('uses the head-at-zero convention (glTF stores 1 - V)', () => {
    expect(patchSnakeVertexShader(standardVertexShader())).toContain('float snakeAlong = 1.0 - uv.y;');
  });

  it('fails loudly if a three.js chunk disappears or repeats', () => {
    expect(() => patchSnakeVertexShader('void main(){}')).toThrow(/expected exactly one/);
    const twice = '#include <common>\n#include <common>\n#include <uv_vertex>\n#include <begin_vertex>';
    expect(() => patchSnakeVertexShader(twice)).toThrow(/expected exactly one/);
  });
});

describe('createSnakeUniforms', () => {
  it('starts neutral: no swell, no pulses, axis along X', () => {
    const uniforms = createSnakeUniforms();
    expect(uniforms.uSwell.value).toBe(0);
    expect(uniforms.uPulses.value).toHaveLength(MAX_PULSES);
    expect(uniforms.uPulses.value.every((pulse) => pulse.y === 0)).toBe(true);
    expect(uniforms.uAxis.value.toArray()).toEqual([1, 0, 0]);
  });
});

describe('normalUvSlopeSign', () => {
  const geometry = (slope: number) => {
    const uv: number[] = [];
    const uv1: number[] = [];
    for (let i = 0; i < 20; i += 1) {
      const v = i / 19; // contract V
      uv.push(0.5, 1 - v); // glTF stores 1 - V
      uv1.push(0.3, slope * (v - 0.5) + 0.5);
    }
    const g = new BufferGeometry();
    g.setAttribute('uv', new Float32BufferAttribute(uv, 2));
    g.setAttribute('uv1', new Float32BufferAttribute(uv1, 2));
    return g;
  };

  it('reads the direction in which the normal-map coordinate changes along the body', () => {
    expect(normalUvSlopeSign(geometry(-5))).toBe(-1);
    expect(normalUvSlopeSign(geometry(5))).toBe(1);
  });

  it('returns 0 when the second UV set is missing', () => {
    const g = new BufferGeometry();
    g.setAttribute('uv', new Float32BufferAttribute([0, 0, 1, 1], 2));
    expect(normalUvSlopeSign(g)).toBe(0);
  });
});

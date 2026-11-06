import { describe, expect, it } from 'vitest';
import {
  BassEnvelope,
  MAX_DT,
  PhaseClock,
  PulseTrain,
  safeSwellLimit,
  speedFromBass,
  swellFromBass,
  wrap01,
} from './snake-phase';

describe('PhaseClock', () => {
  it('stays inside [0, 1) and completes a loop after one period', () => {
    const clock = new PhaseClock(2);
    let wraps = 0;
    let last = 0;
    for (let i = 0; i < 250; i += 1) {
      const phase = clock.advance(0.01, 1);
      expect(phase).toBeGreaterThanOrEqual(0);
      expect(phase).toBeLessThan(1);
      if (phase < last) wraps += 1;
      last = phase;
    }
    expect(wraps).toBe(1);
  });

  it('never jumps when the speed changes: each step is bounded by dt * speed / period', () => {
    const clock = new PhaseClock(4);
    let previous = 0;
    for (let i = 0; i < 400; i += 1) {
      const speed = i % 20 < 10 ? 0.6 : 2;
      const phase = clock.advance(1 / 60, speed);
      const delta = wrap01(phase - previous);
      expect(delta).toBeLessThanOrEqual((1 / 60) * 2 / 4 + 1e-9);
      expect(delta).toBeGreaterThanOrEqual(0);
      previous = phase;
    }
  });

  it('ignores negative or non-finite speeds and clamps long frames', () => {
    const clock = new PhaseClock(1);
    expect(clock.advance(0.05, -3)).toBe(0);
    expect(clock.advance(0.05, Number.NaN)).toBe(0);
    const longFrame = new PhaseClock(1).advance(10, 1);
    expect(longFrame).toBeCloseTo(MAX_DT, 9);
  });
});

describe('BassEnvelope', () => {
  it('rises faster than it falls', () => {
    const env = new BassEnvelope(0.04, 0.28);
    for (let i = 0; i < 6; i += 1) env.update(1, 1 / 60);
    const risen = env.value;
    for (let i = 0; i < 6; i += 1) env.update(0, 1 / 60);
    const fallen = risen - env.value;
    expect(risen).toBeGreaterThan(0.6);
    expect(fallen).toBeLessThan(risen * 0.5);
  });

  it('clamps its input to [0, 1]', () => {
    const env = new BassEnvelope();
    for (let i = 0; i < 100; i += 1) env.update(9, 0.016);
    expect(env.value).toBeLessThanOrEqual(1);
    for (let i = 0; i < 400; i += 1) env.update(-4, 0.016);
    expect(env.value).toBeGreaterThanOrEqual(0);
  });
});

describe('bass mapping', () => {
  it('bounds the speed and the swell', () => {
    expect(speedFromBass(0, 0.6)).toBe(1);
    expect(speedFromBass(1, 0.6)).toBeCloseTo(1.6, 9);
    expect(speedFromBass(1, 50)).toBe(2);
    expect(speedFromBass(1, -50)).toBe(0.6);
    expect(swellFromBass(1, 0.5, 0.12)).toBe(0.12);
    expect(swellFromBass(0.1, 0.5, 0.12)).toBeCloseTo(0.05, 9);
    expect(swellFromBass(-1, 0.5, 0.12)).toBe(0);
  });

  it('derives a swell limit that keeps the tube clear of its bends', () => {
    // radius 0.18, tightest bend radius 0.7: grow until 60% of the bend radius.
    expect(safeSwellLimit(0.18, 0.7)).toBeCloseTo((0.7 * 0.6 - 0.18) / 0.18, 9);
    expect(safeSwellLimit(0.5, 0.4)).toBe(0);
    expect(safeSwellLimit(0, 1)).toBe(0);
  });
});

describe('PulseTrain', () => {
  it('moves pulses from head to tail, drops them past the tail and caps the count', () => {
    const train = new PulseTrain(2, 1);
    train.spawn(1);
    train.spawn(0.5);
    train.spawn(0.25);
    expect(train.pulses).toHaveLength(2);
    expect(train.pulses[0].amplitude).toBe(0.5);
    for (let i = 0; i < 12; i += 1) train.update(0.1, 1);
    expect(train.pulses).toHaveLength(0);
  });

  it('travels faster when the speed rises', () => {
    const slow = new PulseTrain(4, 1);
    const fast = new PulseTrain(4, 1);
    slow.spawn(1);
    fast.spawn(1);
    slow.update(0.1, 1);
    fast.update(0.1, 2);
    expect(fast.pulses[0].position).toBeGreaterThan(slow.pulses[0].position);
  });
});

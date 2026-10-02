import { describe, expect, it } from 'vitest';
import { createMascotAudioResponseState, getMascotArcResponses, updateMascotAudioResponse } from './mascot-audio-response';

const bands = { bassMinHz: 20, bassMaxHz: 250, voiceMinHz: 300, voiceMaxHz: 3500, energyMinHz: 4000, energyMaxHz: 16000, sampleRate: 48000 };

function levelsFor(kind: 'bass' | 'voice' | 'energy') {
  const levels = Array.from({ length: 64 }, () => 0);
  const ranges = { bass: [1, 35], voice: [39, 52], energy: [53, 64] } as const;
  const [start, end] = ranges[kind];
  for (let index = start; index < end; index += 1) levels[index] = 1;
  return levels;
}

function levelsForAmplitude(kind: 'bass' | 'voice' | 'energy', amplitude: number) {
  return levelsFor(kind).map((level) => level * amplitude);
}

function frame(levels: number[], timeMs: number, state = createMascotAudioResponseState()) {
  return updateMascotAudioResponse(state, levels, timeMs, bands);
}

describe('mascot audio response', () => {
  it('uses bass for shared depth while keeping color responses independent', () => {
    const state = createMascotAudioResponseState();
    frame(Array.from({ length: 64 }, () => 0), 0, state);
    const response = frame(levelsFor('bass'), 50, state);
    expect(response.bass).toBeGreaterThan(response.voice);
    expect(response.bass).toBeGreaterThan(response.energy);
    expect(response.beat).toBeGreaterThan(0);
    expect(response.depth).toBe(response.beat);
  });

  it('drives the red response from bass while isolating voice and energy', () => {
    const response = frame(levelsFor('bass'), 0);
    expect(response.bass).toBeGreaterThan(0);
    expect(response.beat).toBe(0);
    expect(response.depth).toBe(0);
    expect(response.voice).toBe(0);
    expect(response.energy).toBe(0);
  });

  it('keeps purple voice response isolated from bass', () => {
    const response = frame(levelsFor('voice'), 0);
    expect(response.voice).toBeGreaterThan(response.bass);
    expect(response.depth).toBe(0);
    expect(response.energy).toBe(0);
  });

  it('keeps green energy response isolated from bass and voice', () => {
    const response = frame(levelsFor('energy'), 0);
    expect(response.energy).toBeGreaterThan(response.bass);
    expect(response.depth).toBe(0);
    expect(response.voice).toBe(0);
  });

  it('stays silent without analyzer data', () => {
    const state = createMascotAudioResponseState();
    expect(frame([], 0, state)).toEqual({ bass: 0, beat: 0, depth: 0, voice: 0, energy: 0 });
    expect(frame([], 100, state)).toEqual({ bass: 0, beat: 0, depth: 0, voice: 0, energy: 0 });
  });

  it('emits a transient beat for a bass onset without copying sustained bass', () => {
    const state = createMascotAudioResponseState();
    frame(Array.from({ length: 64 }, () => 0), 0, state);
    const onset = frame(levelsFor('bass'), 50, state);
    const sustained = frame(levelsFor('bass'), 500, state);
    expect(onset.beat).toBeGreaterThan(0.5);
    expect(onset.depth).toBe(onset.beat);
    expect(sustained.bass).toBeGreaterThan(0.5);
    expect(sustained.beat).toBeLessThan(onset.beat * 0.1);
  });

  it('suppresses small bass jitter instead of turning it into depth flicker', () => {
    const state = createMascotAudioResponseState();
    frame(levelsForAmplitude('bass', 0.5), 0, state);
    const jitter = frame(levelsForAmplitude('bass', 0.53), 50, state);
    const settled = frame(levelsForAmplitude('bass', 0.5), 100, state);
    expect(jitter.depth).toBeLessThan(0.1);
    expect(settled.depth).toBeLessThan(0.1);
  });

  it('keeps near-equal voice and energy arcs bound to their roles when amplitudes cross', () => {
    const state = createMascotAudioResponseState();
    const voiceFirst = frame(levelsForAmplitude('voice', 0.51), 0, state);
    const firstMapping = getMascotArcResponses(voiceFirst);
    expect(firstMapping[1]).toBe(voiceFirst.voice);
    expect(firstMapping[2]).toBe(voiceFirst.energy);

    const energyFirst = frame(levelsForAmplitude('energy', 0.51), 50, state);
    const secondMapping = getMascotArcResponses(energyFirst);
    expect(secondMapping[1]).toBe(energyFirst.voice);
    expect(secondMapping[2]).toBe(energyFirst.energy);
    expect(secondMapping[1]).not.toBe(secondMapping[2]);
  });

  it('keeps beat attack immediate and release time-based', () => {
    const state = createMascotAudioResponseState();
    frame(Array.from({ length: 64 }, () => 0), 0, state);
    const onset = frame(levelsFor('bass'), 50, state);
    const release = frame(levelsFor('bass'), 100, state);
    const settled = frame(levelsFor('bass'), 500, state);
    expect(onset.depth).toBeGreaterThan(0.5);
    expect(release.depth).toBeLessThan(onset.depth);
    expect(settled.depth).toBeLessThan(0.1);
  });

  it('does not move depth for a steady bass sample without an onset', () => {
    const response = frame(levelsFor('bass'), 0);
    expect(response.bass).toBeGreaterThan(0);
    expect(response.depth).toBe(0);
  });

  it('keeps every audio response at zero when analyzer data is absent', () => {
    const response = frame([], 0);
    expect(response).toEqual({ bass: 0, beat: 0, depth: 0, voice: 0, energy: 0 });
  });

  it('produces equivalent envelopes at different frame rates', () => {
    const sixtyFps = createMascotAudioResponseState();
    const thirtyFps = createMascotAudioResponseState();
    let response60 = frame(levelsFor('bass'), 0, sixtyFps);
    let response30 = frame(levelsFor('bass'), 0, thirtyFps);
    for (let time = 1000 / 60; time <= 1000; time += 1000 / 60) response60 = frame(levelsFor('bass'), time, sixtyFps);
    for (let time = 1000 / 30; time <= 1000; time += 1000 / 30) response30 = frame(levelsFor('bass'), time, thirtyFps);
    expect(Math.abs(response60.bass - response30.bass)).toBeLessThan(0.01);
    expect(Math.abs(response60.depth - response30.depth)).toBeLessThan(0.01);
  });
});

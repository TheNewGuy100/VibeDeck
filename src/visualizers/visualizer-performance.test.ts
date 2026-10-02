import { describe, expect, it } from 'vitest';
import { BARS_GRADIENT_TRANSITION_START, capDevicePixelRatio, clampSpectrogramCaptureMs, DEFAULT_SERPENT_RESOLUTION, getBarsGradientPeakMix, getVisualizerPlaybackMode, shouldAnimateVisualizer, shouldCaptureSpectrogram, smoothVisualizerValue, SPECTROGRAM_HISTORY_MS, VISUALIZER_MAX_CANVAS_DPR, VISUALIZER_TARGET_FRAME_MS } from './visualizer-performance';

describe('visualizer performance defaults', () => {
  it('keeps the frame budget at 60 FPS', () => {
    expect(VISUALIZER_TARGET_FRAME_MS).toBeCloseTo(16.666, 2);
  });

  it('uses a medium-high bounded quality default', () => {
    expect(DEFAULT_SERPENT_RESOLUTION).toBe(1.25);
    expect(VISUALIZER_MAX_CANVAS_DPR).toBe(1.5);
    expect(capDevicePixelRatio(3)).toBe(1.5);
    expect(capDevicePixelRatio(0.5)).toBe(1);
  });

  it('keeps spectrogram history bounded', () => {
    expect(SPECTROGRAM_HISTORY_MS).toBe(10_000);
    expect(clampSpectrogramCaptureMs(1)).toBe(10);
    expect(clampSpectrogramCaptureMs(1000)).toBe(100);
    expect(shouldCaptureSpectrogram(false, 100, 20)).toBe(false);
    expect(shouldCaptureSpectrogram(true, 19, 20)).toBe(false);
    expect(shouldCaptureSpectrogram(true, 20, 20)).toBe(true);
  });

  it('keeps no-track background ambient and audio-reactive only while playing', () => {
    expect(getVisualizerPlaybackMode(true, true)).toBe('active');
    expect(getVisualizerPlaybackMode(false, true)).toBe('paused');
    expect(getVisualizerPlaybackMode(false, false)).toBe('idle');
    expect(shouldAnimateVisualizer(true, true, true)).toBe(true);
    expect(shouldAnimateVisualizer(false, true, false, true)).toBe(true);
    expect(shouldAnimateVisualizer(false, false, false, true)).toBe(true);
    expect(shouldAnimateVisualizer(false, true, false, false)).toBe(false);
    expect(shouldAnimateVisualizer(false, false, true)).toBe(false);
    expect(shouldAnimateVisualizer(false, true, true)).toBe(false);
  });

  it('uses the same ambient contract in the paused drawer and playlist background', () => {
    expect(shouldAnimateVisualizer(false, true, false, true)).toBe(true);
    expect(shouldAnimateVisualizer(false, false, false, true)).toBe(true);
    expect(shouldAnimateVisualizer(true, true, false, true)).toBe(true);
    expect(shouldAnimateVisualizer(false, true, true, true)).toBe(false);
  });

  it('smooths bars with responsive attack and slower release', () => {
    const attack = smoothVisualizerValue(0, 1, 33, 0.5);
    const release = smoothVisualizerValue(1, 0, 33, 0.5);
    expect(attack).toBeGreaterThan(0);
    expect(attack).toBeLessThan(1);
    expect(release).toBeGreaterThan(0);
    expect(release).toBeLessThan(1);
    expect(attack).toBeGreaterThan(1 - release);
    expect(smoothVisualizerValue(0.2, 0.9, 0, 0.5)).toBeCloseTo(0.2);
    expect(smoothVisualizerValue(0, 1, 10000, 0.5)).toBeLessThan(1);
  });

  it('maps each bar percentage to a continuous green-to-red peak mix', () => {
    expect(BARS_GRADIENT_TRANSITION_START).toBe(0.5);
    expect(getBarsGradientPeakMix(0.2)).toBe(0);
    expect(getBarsGradientPeakMix(0.5)).toBe(0);
    expect(getBarsGradientPeakMix(0.51)).toBeGreaterThan(0);
    expect(getBarsGradientPeakMix(0.75)).toBeGreaterThan(getBarsGradientPeakMix(0.51));
    expect(getBarsGradientPeakMix(1)).toBeCloseTo(1);
    expect(getBarsGradientPeakMix(2)).toBeCloseTo(1);
  });

  it('keeps the percentage color mapping continuous around the transition', () => {
    const below = getBarsGradientPeakMix(0.59);
    const above = getBarsGradientPeakMix(0.6);
    expect(above - below).toBeLessThan(0.02);
    expect(getBarsGradientPeakMix(0.8)).toBeGreaterThan(getBarsGradientPeakMix(0.7));
  });
});

export const VISUALIZER_TARGET_FRAME_MS = 1000 / 60;
export const VISUALIZER_MAX_CANVAS_DPR = 1.5;
export const DEFAULT_SERPENT_RESOLUTION = 1.25;
export const SPECTROGRAM_HISTORY_MS = 10_000;
export const SPECTROGRAM_MIN_CAPTURE_MS = 10;
export const SPECTROGRAM_MAX_CAPTURE_MS = 100;
export const BARS_GRADIENT_TRANSITION_START = 0.5;
export const BARS_ATTACK_MIN_MS = 24;
export const BARS_ATTACK_MAX_MS = 72;
export const BARS_RELEASE_MIN_MS = 90;
export const BARS_RELEASE_MAX_MS = 280;

export type VisualizerPlaybackMode = 'active' | 'paused' | 'idle';

export function getVisualizerPlaybackMode(active: boolean, hasTrack: boolean): VisualizerPlaybackMode {
  if (active) return 'active';
  return hasTrack ? 'paused' : 'idle';
}

export function shouldAnimateVisualizer(active: boolean, _hasTrack: boolean, reducedMotion: boolean, visible = true) {
  return visible && (active || !reducedMotion);
}

export function capDevicePixelRatio(devicePixelRatio: number | undefined, maximum = VISUALIZER_MAX_CANVAS_DPR) {
  const ratio = Number.isFinite(devicePixelRatio) ? (devicePixelRatio as number) : 1;
  return Math.min(maximum, Math.max(1, ratio));
}

export function clampSpectrogramCaptureMs(value: number) {
  return Math.min(SPECTROGRAM_MAX_CAPTURE_MS, Math.max(SPECTROGRAM_MIN_CAPTURE_MS, Number.isFinite(value) ? value : 20));
}

export function shouldCaptureSpectrogram(active: boolean, elapsedMs: number, captureMs: number) {
  return active && elapsedMs >= clampSpectrogramCaptureMs(captureMs);
}

export function smoothVisualizerValue(current: number, target: number, deltaMs: number, smoothing: number) {
  const boundedCurrent = Math.min(1, Math.max(0, Number.isFinite(current) ? current : 0));
  const boundedTarget = Math.min(1, Math.max(0, Number.isFinite(target) ? target : 0));
  const boundedDelta = Math.min(100, Math.max(0, Number.isFinite(deltaMs) ? deltaMs : 16.67));
  const boundedSmoothing = Math.min(1, Math.max(0, Number.isFinite(smoothing) ? smoothing : 0.5));
  const timeConstant = boundedTarget >= boundedCurrent
    ? BARS_ATTACK_MIN_MS + (BARS_ATTACK_MAX_MS - BARS_ATTACK_MIN_MS) * boundedSmoothing
    : BARS_RELEASE_MIN_MS + (BARS_RELEASE_MAX_MS - BARS_RELEASE_MIN_MS) * boundedSmoothing;
  const factor = 1 - Math.exp(-boundedDelta / timeConstant);
  return boundedCurrent + (boundedTarget - boundedCurrent) * factor;
}

export function getBarsGradientPeakMix(normalizedLevel: number) {
  const level = Math.min(1, Math.max(0, Number.isFinite(normalizedLevel) ? normalizedLevel : 0));
  if (level <= BARS_GRADIENT_TRANSITION_START) return 0;
  const amount = (level - BARS_GRADIENT_TRANSITION_START) / (1 - BARS_GRADIENT_TRANSITION_START);
  return amount * amount * (3 - 2 * amount);
}

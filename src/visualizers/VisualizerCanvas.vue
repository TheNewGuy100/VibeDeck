<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { createMascotAudioResponseState, getMascotArcResponses, updateMascotAudioResponse } from './mascot-audio-response';
import { capDevicePixelRatio, clampSpectrogramCaptureMs, getBarsGradientPeakMix, shouldAnimateVisualizer, shouldCaptureSpectrogram, smoothVisualizerValue, SPECTROGRAM_HISTORY_MS } from './visualizer-performance';

export type VisualizerKind = 'mascot' | 'bars' | 'serpents' | 'spectrogram';

const props = withDefaults(defineProps<{
  kind: VisualizerKind;
  active?: boolean;
  hasTrack?: boolean;
  visible?: boolean;
  compact?: boolean;
  levels?: number[];
  serpentDensity?: number;
  serpentDetail?: number;
  serpentGlow?: number;
  sensitivity?: number;
  smoothing?: number;
  intensity?: number;
  motion?: number;
  bassSensitivity?: number;
  bassDistance?: number;
  bassMinHz?: number;
  bassMaxHz?: number;
  energyMinHz?: number;
  energyMaxHz?: number;
  voiceMinHz?: number;
  voiceMaxHz?: number;
  motionMinHz?: number;
  motionMaxHz?: number;
}>(), { active: false, hasTrack: true, visible: true, compact: false, levels: undefined, serpentDensity: 1, serpentDetail: 1, serpentGlow: 1, sensitivity: 1, smoothing: 0.5, intensity: 1, motion: 1, bassSensitivity: 1, bassDistance: 1, bassMinHz: 20, bassMaxHz: 250, energyMinHz: 20, energyMaxHz: 16000, voiceMinHz: 180, voiceMaxHz: 3500, motionMinHz: 20, motionMaxHz: 250 });

const canvas = ref<HTMLCanvasElement>();
const spectrogramTooltipElement = ref<HTMLElement>();
const spectrogramTooltip = ref<{ x: number; y: number; timeAgo: number; lowHz: number; highHz: number; intensity: number }>();
const spectrogramMinIntensity = ref(0);
const spectrogramMaxIntensity = ref(1);
const spectrogramCaptureMs = ref(20);
const prefersReducedMotion = ref(false);
let reducedMotionQuery: MediaQueryList | undefined;
let handleReducedMotionChange: ((event: MediaQueryListEvent) => void) | undefined;
let animationFrame = 0;
let observer: ResizeObserver | undefined;
let startedAt = performance.now();
let smoothedLevels: number[] = [];
let levelsSource: number[] | undefined;
let barSmoothedLevels: number[] = [];
let barLevelsSource: number[] | undefined;
let lastBarsTime: number | undefined;
const mascotAudioState = createMascotAudioResponseState();
let orbitPhase = 0;
let lastOrbitTime = performance.now();
let lastSpectrogramTime = 0;
let spectrogramHistory: Array<{ levels: number[]; capturedAt: number }> = [];
let context: CanvasRenderingContext2D | undefined;
const barsGradientCache = new Map<string, CanvasGradient>();
let barsGradientHeight = 0;
const serpentPointBuffers: Array<Array<{ x: number; y: number }>> = [];
const EMPTY_LEVELS: number[] = [];

function updateLevels() {
  const target = props.active ? (props.levels ?? EMPTY_LEVELS) : EMPTY_LEVELS;
  if (target === levelsSource) return;
  levelsSource = target;
  if (!target.length) {
    smoothedLevels = [];
    return;
  }
  smoothedLevels = [...target];
}

function updateBarsLevels(time: number) {
  const target = props.active ? (props.levels ?? EMPTY_LEVELS) : EMPTY_LEVELS;
  if (target !== barLevelsSource) {
    barLevelsSource = target;
    if (!target.length) {
      barSmoothedLevels = [];
      lastBarsTime = time;
      return;
    }
    barSmoothedLevels = target.map((level, index) => barSmoothedLevels[index] ?? Math.min(1, Math.max(0, level ?? 0)));
  }
  if (!target.length) return;
  const deltaMs = lastBarsTime === undefined ? 16.67 : time - lastBarsTime;
  lastBarsTime = time;
  for (let index = 0; index < target.length; index += 1) {
    barSmoothedLevels[index] = smoothVisualizerValue(barSmoothedLevels[index] ?? 0, target[index] ?? 0, deltaMs, props.smoothing);
  }
}

function isIdle() {
  return shouldAnimateVisualizer(props.active, props.hasTrack, prefersReducedMotion.value, props.visible) && !props.active;
}

function resize() {
  const element = canvas.value;
  if (!element) return;
  const bounds = element.getBoundingClientRect();
  const ratio = capDevicePixelRatio(window.devicePixelRatio);
  const width = Math.max(1, Math.floor(bounds.width * ratio));
  const height = Math.max(1, Math.floor(bounds.height * ratio));
  if (element.width === width && element.height === height) return;
  element.width = width;
  element.height = height;
  barsGradientCache.clear();
  barsGradientHeight = 0;
}

function mixGradientColor(start: [number, number, number], end: [number, number, number], amount: number) {
  const mix = Math.min(1, Math.max(0, amount));
  return `rgb(${Math.round(start[0] + (end[0] - start[0]) * mix)}, ${Math.round(start[1] + (end[1] - start[1]) * mix)}, ${Math.round(start[2] + (end[2] - start[2]) * mix)})`;
}

function getBarsGradient(context: CanvasRenderingContext2D, baseline: number, barHeight: number, normalizedLevel: number) {
  const heightBucket = Math.max(1, Math.round(barHeight));
  const peakMixBucket = Math.round(getBarsGradientPeakMix(normalizedLevel) * 32);
  const key = `${heightBucket}:${peakMixBucket}`;
  const cached = barsGradientCache.get(key);
  if (cached) return cached;
  if (barsGradientCache.size >= 512) barsGradientCache.clear();
  const peakMix = peakMixBucket / 32;
  const gradient = context.createLinearGradient(0, baseline - heightBucket, 0, baseline);
  gradient.addColorStop(0, mixGradientColor([200, 255, 109], [255, 95, 85], peakMix));
  gradient.addColorStop(0.42, mixGradientColor([185, 243, 107], [255, 95, 85], peakMix * 0.58));
  gradient.addColorStop(0.7, '#75df7d');
  gradient.addColorStop(1, 'rgba(70, 141, 149, .18)');
  barsGradientCache.set(key, gradient);
  return gradient;
}

function drawMascot(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  const centerX = width * 0.5;
  const centerY = height * 0.5;
  const size = Math.min(width, height);
  const audioResponse = updateMascotAudioResponse(mascotAudioState, smoothedLevels, time, {
    bassMinHz: props.bassMinHz,
    bassMaxHz: props.bassMaxHz,
    voiceMinHz: props.voiceMinHz,
    voiceMaxHz: props.voiceMaxHz,
    energyMinHz: props.energyMinHz,
  energyMaxHz: props.energyMaxHz,
  });
  const depthPulse = audioResponse.depth;
  const orbitResponses = getMascotArcResponses(audioResponse);
  const mascotScale = 1 + depthPulse * 0.14 * props.bassSensitivity * props.bassDistance;
  context.lineWidth = Math.max(1.5, size * 0.004);
  context.fillStyle = '#b9f36b';
  const elapsed = Math.min(50, Math.max(0, time - lastOrbitTime));
  lastOrbitTime = time;
  const orbitSpeed = 0.00028 + depthPulse * 0.0008;
  orbitPhase += elapsed * orbitSpeed * props.motion;
  const orbitRotation = orbitPhase;
  const orbitColors = [[255, 95, 85], [193, 166, 255], [185, 243, 107]];
  const sharedDepthScale = 1 + depthPulse * 0.45;
  const orbitRadii = [
    [size * 0.27 * sharedDepthScale, size * 0.15 * sharedDepthScale],
    [size * 0.34 * sharedDepthScale, size * 0.2 * sharedDepthScale],
    [size * 0.43 * sharedDepthScale, size * 0.26 * sharedDepthScale],
  ];
  context.save();
  context.translate(centerX, centerY);
  context.scale(mascotScale, mascotScale);
  context.translate(-centerX, -centerY);
  for (let index = 0; index < 3; index += 1) {
    context.save();
    context.translate(centerX, centerY);
    context.rotate(orbitRotation + index * (Math.PI / 3));
    const response = orbitResponses[index];
    context.globalAlpha = 0.38 + response * 0.5;
    const colorAmount = Math.min(1, Math.max(0, (response - 0.12) / 0.7));
    const peakColor = orbitColors[index];
    const colorMix = colorAmount * props.intensity;
    const red = Math.round(103 + (peakColor[0] - 103) * colorMix);
    const green = Math.round(119 + (peakColor[1] - 119) * colorMix);
    const blue = Math.round(151 + (peakColor[2] - 151) * colorMix);
    context.strokeStyle = `rgb(${red}, ${green}, ${blue})`;
    context.lineWidth = Math.min(size * 0.006, Math.max(1.5, size * (0.003 + response * 0.002 + depthPulse * 0.001)));
    context.beginPath();
    context.ellipse(0, 0, orbitRadii[index][0], orbitRadii[index][1], 0, 0, Math.PI * 2);
    context.stroke();
    context.restore();
  }
  context.globalAlpha = 1;
  const colors = ['#ff5f55', '#c1a6ff', '#b9f36b'];
  [0.11, 0.52, 0.84].forEach((position, index) => {
    const angle = orbitPhase * (index === 0 ? 2.1 : 1.45 + index * 0.12) + index * 2.1;
    const orientation = orbitRotation + index * (Math.PI / 3);
    const depth = (Math.sin(angle) + 1) * 0.5;
    const depthScale = 0.72 + depth * depthPulse * 0.5;
    const localX = Math.cos(angle) * orbitRadii[index][0];
    const localY = Math.sin(angle) * orbitRadii[index][1];
    const x = centerX + localX * Math.cos(orientation) - localY * Math.sin(orientation);
    const y = centerY + localX * Math.sin(orientation) + localY * Math.cos(orientation);
    context.beginPath();
    context.fillStyle = colors[index];
    context.globalAlpha = Math.min(1, (0.45 + depth * 0.55) * props.intensity);
    context.shadowBlur = 8 + depth * depthPulse * 16;
    context.shadowColor = colors[index];
    context.arc(x, y, Math.max(3, Math.min(width, height) * 0.012 * depthScale), 0, Math.PI * 2);
    context.fill();
    context.shadowBlur = 0;
  });
  context.globalAlpha = 1;
  context.restore();
}

function drawBars(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  const count = props.compact ? 28 : 48;
  const gap = Math.max(2, width * 0.008);
  const barWidth = Math.max(2, (width - gap * (count - 1)) / count);
  const baseline = height * 0.8;
  if (barsGradientHeight !== height) {
    barsGradientCache.clear();
    barsGradientHeight = height;
  }
  for (let index = 0; index < count; index += 1) {
    const fallback = Math.sin(time * 0.004 + index * 0.42) * 0.5 + Math.sin(time * 0.0017 + index * 0.13) * 0.3 + 0.2;
    const spectrumPosition = barSmoothedLevels.length > 1 ? index * (barSmoothedLevels.length - 1) / (count - 1) : 0;
    const lowerIndex = Math.floor(spectrumPosition);
    const upperIndex = Math.min(barSmoothedLevels.length - 1, lowerIndex + 1);
    const fraction = spectrumPosition - lowerIndex;
    const spectrumLevel = barSmoothedLevels.length ? (barSmoothedLevels[lowerIndex] ?? 0) + ((barSmoothedLevels[upperIndex] ?? 0) - (barSmoothedLevels[lowerIndex] ?? 0)) * fraction : 0;
    const level = barSmoothedLevels.length ? spectrumLevel * 1.8 : fallback * props.motion;
    const normalizedLevel = Math.min(1, Math.max(0, level));
    const adjustedLevel = Math.max(0.12, normalizedLevel) * (props.active ? 1 : 0.45);
    const barHeight = Math.min(height * 0.76, height * (0.12 + adjustedLevel * 0.62 * props.intensity));
    const x = index * (barWidth + gap);
    context.fillStyle = getBarsGradient(context, baseline, barHeight, normalizedLevel);
    context.beginPath();
    context.roundRect(x, baseline - barHeight, barWidth, barHeight, barWidth * 0.5);
    context.fill();
  }
}

function drawSerpents(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  const centerX = width * 0.5;
  const centerY = height * 0.5;
  const size = Math.min(width, height);
  const bassCount = Math.max(2, Math.floor(smoothedLevels.length * 0.14));
  let bass = 0;
  for (let index = 0; index < Math.min(bassCount, smoothedLevels.length); index += 1) bass += smoothedLevels[index] ?? 0;
  bass = smoothedLevels.length ? bass / Math.min(bassCount, smoothedLevels.length) : 0;
  const pulse = Math.min(1, bass * 1.8);
  const layers = props.compact ? 2 : 2;
  const serpentsPerLayer = Math.round((props.compact ? 5 : 9) * props.serpentDensity);
  const span = Math.max(width, height);
  const background = context.createRadialGradient(centerX, centerY, size * 0.08, centerX, centerY, size * 0.78);
  background.addColorStop(0, '#010204');
  background.addColorStop(0.35, `rgba(74, 4, 12, ${(0.08 + pulse * 0.2) * props.serpentGlow})`);
  background.addColorStop(1, '#010204');
  context.fillStyle = background;
  context.fillRect(0, 0, width, height);

  for (let layer = 0; layer < layers; layer += 1) {
    const innerRadius = size * (0.16 + layer * 0.055);
    const reach = span * (0.58 + pulse * 0.18 - layer * 0.06);
    const serpentCount = serpentsPerLayer + layer;
    for (let serpent = 0; serpent < serpentCount; serpent += 1) {
      const baseAngle = serpent / serpentCount * Math.PI * 2 + layer * Math.PI / serpentCount + Math.sin(serpent * 9.17 + layer * 3.4) * 0.18;
      const turnDirection = serpent % 2 === 0 ? 1 : -1;
      const totalTurn = turnDirection * (0.52 + (serpent % 3) * 0.16 + layer * 0.12);
      const segments = Math.round((props.compact ? 13 : 24) * props.serpentDetail);
      const bufferIndex = layer * 16 + serpent;
      const points = serpentPointBuffers[bufferIndex] ?? (serpentPointBuffers[bufferIndex] = []);
      points.length = segments + 1;
      for (let segment = 0; segment <= segments; segment += 1) {
        const progress = segment / segments;
        const phase = time * (0.0012 + layer * 0.00025) + serpent * 0.9;
        const wave = Math.sin(phase + progress * Math.PI * 3.2) * size * (0.025 + pulse * 0.018) * (0.35 + progress);
        const distance = innerRadius + reach * progress + Math.sin(phase * 0.7 + progress * Math.PI * 2) * size * 0.025 * pulse;
        const angle = baseAngle + totalTurn * progress + Math.sin(phase * 0.45 + progress * Math.PI * 2.4) * 0.12 + wave / size;
        const point = points[segment] ?? (points[segment] = { x: 0, y: 0 });
        point.x = centerX + Math.cos(angle) * distance;
        point.y = centerY + Math.sin(angle) * distance;
      }

      context.beginPath();
      context.moveTo(points[0].x, points[0].y);
      for (let pointIndex = 1; pointIndex < points.length; pointIndex += 1) context.lineTo(points[pointIndex].x, points[pointIndex].y);
      context.strokeStyle = layer === 0 ? 'rgba(5, 7, 10, .96)' : 'rgba(24, 26, 31, .8)';
      context.lineWidth = size * (0.045 - layer * 0.009 + pulse * 0.012);
      context.lineCap = 'round';
      context.stroke();
      context.strokeStyle = `rgba(108, 112, 119, ${(0.2 + pulse * 0.24 - layer * 0.04) * props.serpentGlow})`;
      context.lineWidth = size * (0.029 - layer * 0.006 + pulse * 0.007);
      context.stroke();

      for (let segment = 2; segment < points.length; segment += 1) {
        const point = points[segment];
        const previous = points[segment - 1];
        const direction = Math.atan2(point.y - previous.y, point.x - previous.x);
        const scaleSize = size * (0.014 - layer * 0.001 + pulse * 0.003);
        const texturePhase = Math.sin(time * 0.0018 + segment * 1.4 + serpent * 0.7 + layer);
        context.save();
        context.translate(point.x, point.y);
        context.rotate(direction + Math.PI / 2);
        context.globalAlpha = 0.55 + Math.max(0, texturePhase) * 0.35;
        context.beginPath();
        context.moveTo(-scaleSize, 0);
        context.quadraticCurveTo(0, scaleSize * 0.9, scaleSize, 0);
        context.lineTo(0, -scaleSize * 0.85);
        context.closePath();
        context.fillStyle = layer === 0 ? 'rgba(28, 31, 36, .96)' : 'rgba(9, 12, 17, .96)';
        context.fill();
        context.strokeStyle = `rgba(213, ${48 + pulse * 32}, ${53 + pulse * 24}, ${(0.12 + pulse * 0.3) * props.serpentGlow})`;
        context.lineWidth = Math.max(1, size * 0.0012);
        context.stroke();
        context.restore();
      }
    }
  }

  context.globalAlpha = 1;
  context.fillStyle = '#000105';
  context.beginPath();
  context.arc(centerX, centerY, size * 0.12, 0, Math.PI * 2);
  context.fill();
  context.strokeStyle = `rgba(151, 23, 31, ${(0.2 + pulse * 0.5) * props.serpentGlow})`;
  context.lineWidth = Math.max(1.5, size * 0.003);
  context.stroke();
}

function drawSpectrogram(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  context.fillStyle = '#03070b';
  context.fillRect(0, 0, width, height);
  if (isIdle()) {
    const sweep = (Math.sin(time * 0.00045) + 1) * 0.5;
    const glow = context.createLinearGradient(width * sweep - width * 0.18, 0, width * sweep + width * 0.18, 0);
    glow.addColorStop(0, 'rgba(90, 220, 190, 0)');
    glow.addColorStop(0.5, 'rgba(90, 220, 190, 0.12)');
    glow.addColorStop(1, 'rgba(90, 220, 190, 0)');
    context.fillStyle = glow;
    context.fillRect(0, 0, width, height);
  }
  const captureMs = clampSpectrogramCaptureMs(spectrogramCaptureMs.value);
  if (shouldCaptureSpectrogram(props.active, time - lastSpectrogramTime, captureMs)) {
    const column = smoothedLevels.length ? smoothedLevels.map((level) => Math.min(1, level * props.sensitivity)) : [];
    if (column.length) {
      spectrogramHistory.push({ levels: column, capturedAt: time });
      const oldestAllowed = time - SPECTROGRAM_HISTORY_MS;
      while (spectrogramHistory.length && spectrogramHistory[0].capturedAt < oldestAllowed) spectrogramHistory.shift();
      lastSpectrogramTime = time;
    }
  }
  if (!spectrogramHistory.length) return;
  const columnWidth = width / spectrogramHistory.length;
  const rowHeight = height / spectrogramHistory[0].levels.length;
  spectrogramHistory.forEach((column, columnIndex) => {
    column.levels.forEach((level, rowIndex) => {
      const rawEnergy = Math.min(1, level * props.intensity);
      const energy = Math.min(1, Math.max(0, (rawEnergy - spectrogramMinIntensity.value) / Math.max(0.01, spectrogramMaxIntensity.value - spectrogramMinIntensity.value)));
      const hue = 195 - (rowIndex / column.levels.length) * 210;
      const alpha = 0.08 + energy * 0.92;
      context.fillStyle = `hsla(${hue}, 88%, ${35 + energy * 35}%, ${alpha})`;
      context.fillRect(columnIndex * columnWidth, height - (rowIndex + 1) * rowHeight, Math.ceil(columnWidth + 1), Math.ceil(rowHeight + 1));
    });
  });
}

function render() {
  const element = canvas.value;
  if (!element) return;
  if (!context) context = element.getContext('2d') ?? undefined;
  if (!context) return;
  const width = element.width;
  const height = element.height;
  const time = performance.now() - startedAt;
  updateLevels();
  if (props.kind === 'bars') updateBarsLevels(time);
  context.clearRect(0, 0, width, height);
  if (props.kind === 'bars') drawBars(context, width, height, time);
  else if (props.kind === 'serpents') drawSerpents(context, width, height, time);
  else if (props.kind === 'spectrogram') drawSpectrogram(context, width, height, time);
  else drawMascot(context, width, height, time);
  if (props.visible && (props.active || isIdle())) animationFrame = requestAnimationFrame(render);
}

function formatFrequency(frequency: number) {
  return frequency >= 1000 ? `${(frequency / 1000).toFixed(1)} kHz` : `${Math.round(frequency)} Hz`;
}

function handleSpectrogramPointerMove(event: PointerEvent) {
  if (props.kind !== 'spectrogram' || !spectrogramHistory.length || !canvas.value) return;
  const bounds = canvas.value.getBoundingClientRect();
  if (!bounds.width || !bounds.height) return;
  const x = Math.max(0, Math.min(bounds.width - 1, event.clientX - bounds.left));
  const y = Math.max(0, Math.min(bounds.height - 1, event.clientY - bounds.top));
  const columnIndex = Math.min(spectrogramHistory.length - 1, Math.floor((x / bounds.width) * spectrogramHistory.length));
  const sample = spectrogramHistory[columnIndex];
  const rowIndex = Math.min(sample.levels.length - 1, Math.floor((1 - y / bounds.height) * sample.levels.length));
  const lowHz = 20 * Math.pow(24000 / 20, rowIndex / sample.levels.length);
  const highHz = 20 * Math.pow(24000 / 20, (rowIndex + 1) / sample.levels.length);
  const latestCapture = spectrogramHistory[spectrogramHistory.length - 1].capturedAt;
  const tooltipWidth = spectrogramTooltipElement.value?.offsetWidth || 164;
  const tooltipHeight = spectrogramTooltipElement.value?.offsetHeight || 70;
  const clamp = (value: number, minimum: number, maximum: number) => Math.min(Math.max(value, minimum), Math.max(minimum, maximum));
  const xPosition = clamp(event.clientX + 14, 8, window.innerWidth - tooltipWidth - 8);
  const yPosition = clamp(event.clientY + 14, 8, window.innerHeight - tooltipHeight - 8);
  spectrogramTooltip.value = {
    x: xPosition,
    y: yPosition,
    timeAgo: Math.max(0, (latestCapture - sample.capturedAt) / 1000),
    lowHz,
    highHz,
    intensity: Math.min(1, sample.levels[rowIndex] * props.intensity),
  };
  requestAnimationFrame(() => {
    if (!spectrogramTooltip.value || !spectrogramTooltipElement.value) return;
    const rect = spectrogramTooltipElement.value.getBoundingClientRect();
    spectrogramTooltip.value.x = clamp(spectrogramTooltip.value.x, 8, window.innerWidth - rect.width - 8);
    spectrogramTooltip.value.y = clamp(spectrogramTooltip.value.y, 8, window.innerHeight - rect.height - 8);
  });
}

function hideSpectrogramTooltip() {
  spectrogramTooltip.value = undefined;
}

watch(() => props.kind, () => {
  startedAt = performance.now();
  lastSpectrogramTime = 0;
  spectrogramHistory = [];
  spectrogramTooltip.value = undefined;
  barLevelsSource = undefined;
  barSmoothedLevels = [];
  lastBarsTime = undefined;
});

watch(() => props.active, (active) => {
  cancelAnimationFrame(animationFrame);
  animationFrame = 0;
  if (!active) {
    spectrogramHistory = [];
    lastSpectrogramTime = 0;
  }
  if (props.visible && (active || isIdle())) animationFrame = requestAnimationFrame(render);
  else render();
});

watch([() => props.hasTrack, prefersReducedMotion], () => {
  cancelAnimationFrame(animationFrame);
  animationFrame = 0;
  if (props.visible && (props.active || isIdle())) animationFrame = requestAnimationFrame(render);
  else render();
});

onMounted(() => {
  resize();
  reducedMotionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
  prefersReducedMotion.value = reducedMotionQuery.matches;
  handleReducedMotionChange = (event: MediaQueryListEvent) => { prefersReducedMotion.value = event.matches; };
  reducedMotionQuery.addEventListener?.('change', handleReducedMotionChange);
  context = canvas.value?.getContext('2d') ?? undefined;
  observer = new ResizeObserver(resize);
  if (canvas.value) observer.observe(canvas.value);
  if (props.visible) animationFrame = requestAnimationFrame(render);
});

onBeforeUnmount(() => {
  cancelAnimationFrame(animationFrame);
  observer?.disconnect();
  if (reducedMotionQuery && handleReducedMotionChange) reducedMotionQuery.removeEventListener?.('change', handleReducedMotionChange);
});
</script>

<template>
  <div v-bind="$attrs" class="visualizer-canvas-shell">
    <canvas ref="canvas" class="visualizer-canvas" aria-hidden="true" @pointermove="handleSpectrogramPointerMove" @pointerleave="hideSpectrogramTooltip" />
    <div v-if="kind === 'spectrogram' && !compact" class="spectrogram-intensity-control">
      <strong>Faixa de intensidade</strong>
      <label><span>Mínimo <b>{{ Math.round(spectrogramMinIntensity * 100) }}%</b></span><input v-model.number="spectrogramMinIntensity" type="range" min="0" :max="Math.max(0, spectrogramMaxIntensity - 0.01)" step="0.01" /></label>
      <label><span>Máximo <b>{{ Math.round(spectrogramMaxIntensity * 100) }}%</b></span><input v-model.number="spectrogramMaxIntensity" type="range" :min="Math.min(0.99, spectrogramMinIntensity + 0.01)" max="1" step="0.01" /></label>
      <label><span>Captura <b>{{ spectrogramCaptureMs }} ms</b></span><input v-model.number="spectrogramCaptureMs" type="range" min="10" max="100" step="5" /></label>
    </div>
  </div>
  <Teleport to="body">
    <div v-if="spectrogramTooltip && kind === 'spectrogram'" ref="spectrogramTooltipElement" class="spectrogram-tooltip" :style="{ left: `${spectrogramTooltip.x}px`, top: `${spectrogramTooltip.y}px` }">
      <strong>{{ spectrogramTooltip.timeAgo < 0.01 ? 'Agora' : `-${spectrogramTooltip.timeAgo.toFixed(2)} s` }}</strong>
      <span>{{ formatFrequency(spectrogramTooltip.lowHz) }} - {{ formatFrequency(spectrogramTooltip.highHz) }}</span>
      <span>Intensidade: {{ Math.round(spectrogramTooltip.intensity * 100) }}%</span>
    </div>
  </Teleport>
</template>

<style scoped>
.spectrogram-tooltip { position: fixed; z-index: 20; min-width: 142px; padding: 8px 10px; border: 1px solid rgba(185, 243, 107, .45); border-radius: 7px; color: #e9f5df; background: rgba(7, 12, 15, .94); box-shadow: 0 8px 24px rgba(0, 0, 0, .3); pointer-events: none; font-size: 10px; line-height: 1.45; }
.spectrogram-tooltip strong, .spectrogram-tooltip span { display: block; }
.spectrogram-tooltip strong { color: #b9f36b; font-size: 11px; }
.spectrogram-tooltip span { color: rgba(233, 245, 223, .72); }
.visualizer-canvas-shell { width: 100%; height: 100%; min-height: 0; position: relative; }
:global(.playlist-visualizer.visualizer-canvas-shell) { position: fixed !important; top: 72px; right: 0; bottom: 92px; left: 244px; z-index: 0; width: auto; height: auto; margin: 0; opacity: .18; pointer-events: none; }
.spectrogram-intensity-control { width: 190px; padding: 9px 11px; position: absolute; top: 14px; left: 14px; z-index: 4; border: 1px solid rgba(255, 255, 255, .14); border-radius: 9px; color: rgba(255, 255, 255, .82); background: rgba(5, 8, 12, .78); backdrop-filter: blur(10px); font-size: 10px; }
.spectrogram-intensity-control strong { display: block; margin-bottom: 8px; color: #b9f36b; }
.spectrogram-intensity-control label, .spectrogram-intensity-control span { display: grid; gap: 4px; }
.spectrogram-intensity-control label + label { margin-top: 8px; }
.spectrogram-intensity-control span { display: flex; justify-content: space-between; }
.spectrogram-intensity-control b { color: #b9f36b; }
.spectrogram-intensity-control input { width: 100%; accent-color: #b9f36b; }
</style>

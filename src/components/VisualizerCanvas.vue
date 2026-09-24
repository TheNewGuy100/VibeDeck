<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';

export type VisualizerKind = 'mascot' | 'bars' | 'serpents' | 'spectrogram';

const props = withDefaults(defineProps<{
  kind: VisualizerKind;
  active?: boolean;
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
}>(), { active: true, compact: false, levels: undefined, serpentDensity: 1, serpentDetail: 1, serpentGlow: 1, sensitivity: 1, smoothing: 0.5, intensity: 1, motion: 1, bassSensitivity: 1, bassDistance: 1, bassMinHz: 20, bassMaxHz: 250, energyMinHz: 20, energyMaxHz: 16000, voiceMinHz: 180, voiceMaxHz: 3500, motionMinHz: 20, motionMaxHz: 250 });

const canvas = ref<HTMLCanvasElement>();
const spectrogramTooltip = ref<{ x: number; y: number; timeAgo: number; lowHz: number; highHz: number; intensity: number }>();
const spectrogramMinIntensity = ref(0);
const spectrogramMaxIntensity = ref(1);
const spectrogramCaptureMs = ref(20);
let animationFrame = 0;
let observer: ResizeObserver | undefined;
let startedAt = performance.now();
let smoothedLevels: number[] = [];
let smoothedOrbitResponses = [0.35, 0.35, 0.35];
let smoothedBeatPulse = 0.35;
let smoothedMotionPulse = 0.35;
let orbitPhase = 0;
let lastOrbitTime = performance.now();
let lastSpectrogramTime = 0;
let spectrogramHistory: number[][] = [];

function updateLevels() {
  const target = props.levels ?? [];
  if (!target.length) {
    smoothedLevels = [];
    return;
  }
  smoothedLevels = [...target];
}

function resize() {
  const element = canvas.value;
  if (!element) return;
  const bounds = element.getBoundingClientRect();
  const ratio = window.devicePixelRatio || 1;
  element.width = Math.max(1, Math.floor(bounds.width * ratio));
  element.height = Math.max(1, Math.floor(bounds.height * ratio));
}

function drawMascot(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  const centerX = width * 0.5;
  const centerY = height * 0.5;
  const size = Math.min(width, height);
  const bandAverage = (startRatio: number, endRatio: number) => {
    if (!smoothedLevels.length) return 0.35;
    const start = Math.floor(smoothedLevels.length * startRatio);
    const end = Math.max(start + 1, Math.floor(smoothedLevels.length * endRatio));
    const band = smoothedLevels.slice(start, end);
    return band.reduce((sum, value) => sum + value, 0) / band.length;
  };
  const bandAverageHz = (lowHz: number, highHz: number) => {
    const nyquist = 24000;
    const toRatio = (frequency: number) => Math.log(Math.max(1, Math.min(nyquist, frequency))) / Math.log(nyquist);
    return bandAverage(toRatio(lowHz), toRatio(highHz));
  };
  const bassEnergy = bandAverageHz(props.bassMinHz, props.bassMaxHz);
  const motionEnergy = bandAverageHz(props.motionMinHz, props.motionMaxHz);
  const orbitEnergies = [
    bassEnergy,
    bandAverageHz(props.energyMinHz, props.energyMaxHz),
    bandAverageHz(props.voiceMinHz, props.voiceMaxHz),
  ];
  const minimumEnergy = Math.min(...orbitEnergies);
  const maximumEnergy = Math.max(...orbitEnergies);
  const energyRange = maximumEnergy - minimumEnergy;
  const orbitResponses = orbitEnergies.map((energy) => {
    const relative = energyRange > 0.012 ? (energy - minimumEnergy) / energyRange : energy;
    return Math.min(1, energy * 0.35 + relative * 0.65);
  });
  smoothedOrbitResponses = orbitResponses.map((response, index) => smoothedOrbitResponses[index] + (response - smoothedOrbitResponses[index]) * 0.12);
  const beatPulseTarget = Math.min(1, bassEnergy * 1.6);
  smoothedBeatPulse += (beatPulseTarget - smoothedBeatPulse) * 0.12;
  const beatPulse = smoothedBeatPulse;
  const motionPulseTarget = Math.min(1, motionEnergy * 1.6);
  smoothedMotionPulse += (motionPulseTarget - smoothedMotionPulse) * 0.12;
  const mascotScale = 1 + smoothedMotionPulse * 0.14 * props.bassSensitivity * props.bassDistance;
  context.lineWidth = Math.max(1.5, size * 0.004);
  context.fillStyle = '#b9f36b';
  const elapsed = Math.min(50, Math.max(0, time - lastOrbitTime));
  lastOrbitTime = time;
  const orbitSpeed = 0.00028 + beatPulse * 0.0008;
  orbitPhase += elapsed * orbitSpeed * props.motion;
  const orbitRotation = orbitPhase;
  const orbitColors = [[255, 95, 85], [185, 243, 107], [193, 166, 255]];
  const bassOrbitScale = 1 + beatPulse * 0.45;
  const orbitRadii = [
    [Math.min(size * 0.34, size * 0.27 * bassOrbitScale), Math.min(size * 0.23, size * 0.15 * bassOrbitScale)],
    [Math.min(size * 0.43, size * 0.34), Math.min(size * 0.3, size * 0.2)],
    [Math.min(size * 0.52, size * 0.43), Math.min(size * 0.37, size * 0.26)],
  ];
  context.save();
  context.translate(centerX, centerY);
  context.scale(mascotScale, mascotScale);
  context.translate(-centerX, -centerY);
  for (let index = 0; index < 3; index += 1) {
    context.save();
    context.translate(centerX, centerY);
    context.rotate(orbitRotation + index * (Math.PI / 3));
    const response = smoothedOrbitResponses[index];
    context.globalAlpha = 0.38 + response * 0.5;
    const colorAmount = Math.min(1, Math.max(0, (response - 0.12) / 0.7));
    const peakColor = orbitColors[index];
    const colorMix = colorAmount * props.intensity;
    const red = Math.round(103 + (peakColor[0] - 103) * colorMix);
    const green = Math.round(119 + (peakColor[1] - 119) * colorMix);
    const blue = Math.round(151 + (peakColor[2] - 151) * colorMix);
    context.strokeStyle = `rgb(${red}, ${green}, ${blue})`;
    context.lineWidth = Math.min(size * 0.006, Math.max(1.5, size * (0.003 + response * 0.002 + beatPulse * 0.001)));
    context.beginPath();
    context.ellipse(0, 0, orbitRadii[index][0], orbitRadii[index][1], 0, 0, Math.PI * 2);
    context.stroke();
    context.restore();
  }
  context.globalAlpha = 1;
  const colors = ['#ff5f55', '#b9f36b', '#c1a6ff'];
  [0.11, 0.52, 0.84].forEach((position, index) => {
    const angle = orbitPhase * (index === 0 ? 2.1 : 1.45 + index * 0.12) + index * 2.1;
    const orientation = orbitRotation + index * (Math.PI / 3);
    const depth = (Math.sin(angle) + 1) * 0.5;
    const depthScale = 0.72 + depth * beatPulse * 0.5;
    const localX = Math.cos(angle) * orbitRadii[index][0];
    const localY = Math.sin(angle) * orbitRadii[index][1];
    const x = centerX + localX * Math.cos(orientation) - localY * Math.sin(orientation);
    const y = centerY + localX * Math.sin(orientation) + localY * Math.cos(orientation);
    context.beginPath();
    context.fillStyle = colors[index];
    context.globalAlpha = Math.min(1, (0.45 + depth * 0.55) * props.intensity);
    context.shadowBlur = 8 + depth * beatPulse * 16;
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
  for (let index = 0; index < count; index += 1) {
    const fallback = Math.sin(time * 0.004 + index * 0.42) * 0.5 + Math.sin(time * 0.0017 + index * 0.13) * 0.3 + 0.2;
    const spectrumPosition = smoothedLevels.length > 1 ? index * (smoothedLevels.length - 1) / (count - 1) : 0;
    const lowerIndex = Math.floor(spectrumPosition);
    const upperIndex = Math.min(smoothedLevels.length - 1, lowerIndex + 1);
    const fraction = spectrumPosition - lowerIndex;
    const spectrumLevel = smoothedLevels.length ? (smoothedLevels[lowerIndex] ?? 0) + ((smoothedLevels[upperIndex] ?? 0) - (smoothedLevels[lowerIndex] ?? 0)) * fraction : 0;
    const level = smoothedLevels.length ? spectrumLevel * 1.8 : fallback * props.motion;
    const normalizedLevel = Math.min(1, Math.max(0, level));
    const adjustedLevel = Math.max(0.12, normalizedLevel) * (props.active ? 1 : 0.45);
    const barHeight = Math.min(height * 0.76, height * (0.12 + adjustedLevel * 0.62 * props.intensity));
    const x = index * (barWidth + gap);
    const gradient = context.createLinearGradient(0, baseline - barHeight, 0, baseline);
    const nearLimit = normalizedLevel >= 0.78;
    gradient.addColorStop(0, nearLimit ? '#ff956b' : '#c8ff6d');
    gradient.addColorStop(0.55, '#75df7d');
    gradient.addColorStop(1, 'rgba(70, 141, 149, .18)');
    context.fillStyle = gradient;
    context.beginPath();
    context.roundRect(x, baseline - barHeight, barWidth, barHeight, barWidth * 0.5);
    context.fill();
  }
}

function drawSerpents(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  const centerX = width * 0.5;
  const centerY = height * 0.5;
  const size = Math.min(width, height);
  const bass = smoothedLevels.length
    ? smoothedLevels.slice(0, Math.max(2, Math.floor(smoothedLevels.length * 0.14))).reduce((sum, value) => sum + value, 0) / Math.max(2, Math.floor(smoothedLevels.length * 0.14))
    : 0.35;
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
      const points: Array<{ x: number; y: number }> = [];
      for (let segment = 0; segment <= segments; segment += 1) {
        const progress = segment / segments;
        const phase = time * (0.0012 + layer * 0.00025) + serpent * 0.9;
        const wave = Math.sin(phase + progress * Math.PI * 3.2) * size * (0.025 + pulse * 0.018) * (0.35 + progress);
        const distance = innerRadius + reach * progress + Math.sin(phase * 0.7 + progress * Math.PI * 2) * size * 0.025 * pulse;
        const angle = baseAngle + totalTurn * progress + Math.sin(phase * 0.45 + progress * Math.PI * 2.4) * 0.12 + wave / size;
        points.push({ x: centerX + Math.cos(angle) * distance, y: centerY + Math.sin(angle) * distance });
      }

      context.beginPath();
      context.moveTo(points[0].x, points[0].y);
      points.slice(1).forEach((point) => context.lineTo(point.x, point.y));
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
  if (time - lastSpectrogramTime >= spectrogramCaptureMs.value) {
    const column = smoothedLevels.length ? smoothedLevels.map((level) => Math.min(1, level * props.sensitivity)) : [];
    if (props.active && column.length) spectrogramHistory.push(column);
    const maxColumns = Math.ceil(10000 / spectrogramCaptureMs.value);
    if (spectrogramHistory.length > maxColumns) spectrogramHistory = spectrogramHistory.slice(-maxColumns);
    lastSpectrogramTime = time;
  }
  if (!spectrogramHistory.length) return;
  const columnWidth = width / spectrogramHistory.length;
  const rowHeight = height / spectrogramHistory[0].length;
  spectrogramHistory.forEach((column, columnIndex) => {
    column.forEach((level, rowIndex) => {
      const rawEnergy = Math.min(1, level * props.intensity);
      const energy = Math.min(1, Math.max(0, (rawEnergy - spectrogramMinIntensity.value) / Math.max(0.01, spectrogramMaxIntensity.value - spectrogramMinIntensity.value)));
      const hue = 195 - (rowIndex / column.length) * 210;
      const alpha = 0.08 + energy * 0.92;
      context.fillStyle = `hsla(${hue}, 88%, ${35 + energy * 35}%, ${alpha})`;
      context.fillRect(columnIndex * columnWidth, height - (rowIndex + 1) * rowHeight, Math.ceil(columnWidth + 1), Math.ceil(rowHeight + 1));
    });
  });
}

function render() {
  const element = canvas.value;
  if (!element) return;
  const context = element.getContext('2d');
  if (!context) return;
  const width = element.width;
  const height = element.height;
  const time = performance.now() - startedAt;
  updateLevels();
  context.clearRect(0, 0, width, height);
  if (props.kind === 'bars') drawBars(context, width, height, time);
  else if (props.kind === 'serpents') drawSerpents(context, width, height, time);
  else if (props.kind === 'spectrogram') drawSpectrogram(context, width, height, time);
  else drawMascot(context, width, height, time);
  animationFrame = requestAnimationFrame(render);
}

function formatFrequency(frequency: number) {
  return frequency >= 1000 ? `${(frequency / 1000).toFixed(1)} kHz` : `${Math.round(frequency)} Hz`;
}

function handleSpectrogramPointerMove(event: PointerEvent) {
  if (props.kind !== 'spectrogram' || !spectrogramHistory.length || !canvas.value) return;
  const bounds = canvas.value.getBoundingClientRect();
  const x = Math.max(0, Math.min(bounds.width - 1, event.clientX - bounds.left));
  const y = Math.max(0, Math.min(bounds.height - 1, event.clientY - bounds.top));
  const columnIndex = Math.min(spectrogramHistory.length - 1, Math.floor((x / bounds.width) * spectrogramHistory.length));
  const column = spectrogramHistory[columnIndex];
  const rowIndex = Math.min(column.length - 1, Math.floor((1 - y / bounds.height) * column.length));
  const lowHz = 20 * Math.pow(24000 / 20, rowIndex / column.length);
  const highHz = 20 * Math.pow(24000 / 20, (rowIndex + 1) / column.length);
  spectrogramTooltip.value = {
    x: event.clientX + 14,
    y: event.clientY + 14,
    timeAgo: (spectrogramHistory.length - 1 - columnIndex) * (spectrogramCaptureMs.value / 1000),
    lowHz,
    highHz,
    intensity: Math.min(1, column[rowIndex] * props.intensity),
  };
}

function hideSpectrogramTooltip() {
  spectrogramTooltip.value = undefined;
}

watch(() => props.kind, () => {
  startedAt = performance.now();
  lastSpectrogramTime = 0;
  spectrogramHistory = [];
  spectrogramTooltip.value = undefined;
});

onMounted(() => {
  resize();
  observer = new ResizeObserver(resize);
  if (canvas.value) observer.observe(canvas.value);
  animationFrame = requestAnimationFrame(render);
});

onBeforeUnmount(() => {
  cancelAnimationFrame(animationFrame);
  observer?.disconnect();
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
    <div v-if="spectrogramTooltip && kind === 'spectrogram'" class="spectrogram-tooltip" :style="{ left: `${spectrogramTooltip.x}px`, top: `${spectrogramTooltip.y}px` }">
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

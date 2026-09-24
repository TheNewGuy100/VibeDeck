<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';

export type VisualizerKind = 'mascot' | 'bars' | 'serpents';

const props = withDefaults(defineProps<{
  kind: VisualizerKind;
  active?: boolean;
  compact?: boolean;
  levels?: number[];
  serpentDensity?: number;
  serpentDetail?: number;
  serpentGlow?: number;
}>(), { active: true, compact: false, levels: undefined, serpentDensity: 1, serpentDetail: 1, serpentGlow: 1 });

const canvas = ref<HTMLCanvasElement>();
let animationFrame = 0;
let observer: ResizeObserver | undefined;
let startedAt = performance.now();
let smoothedLevels: number[] = [];
let smoothedOrbitResponses = [0.35, 0.35, 0.35];
let smoothedBeatPulse = 0.35;
let orbitPhase = 0;
let lastOrbitTime = performance.now();

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
  const orbitEnergies = [0, 1, 2].map((index) => {
    if (!smoothedLevels.length) return 0.35;
    const start = Math.floor(index * smoothedLevels.length / 3);
    const end = Math.max(start + 1, Math.floor((index + 1) * smoothedLevels.length / 3));
    const band = smoothedLevels.slice(start, end);
    return band.reduce((sum, value) => sum + value, 0) / band.length;
  });
  const minimumEnergy = Math.min(...orbitEnergies);
  const maximumEnergy = Math.max(...orbitEnergies);
  const energyRange = maximumEnergy - minimumEnergy;
  const orbitResponses = orbitEnergies.map((energy) => {
    const relative = energyRange > 0.012 ? (energy - minimumEnergy) / energyRange : energy;
    return Math.min(1, energy * 0.35 + relative * 0.65);
  });
  smoothedOrbitResponses = orbitResponses.map((response, index) => smoothedOrbitResponses[index] + (response - smoothedOrbitResponses[index]) * 0.12);
  const bassEnergy = smoothedLevels.length
    ? smoothedLevels.slice(0, Math.max(2, Math.floor(smoothedLevels.length * 0.14))).reduce((sum, value) => sum + value, 0) / Math.max(2, Math.floor(smoothedLevels.length * 0.14))
    : 0.35;
  const beatPulseTarget = Math.min(1, bassEnergy * 1.6);
  smoothedBeatPulse += (beatPulseTarget - smoothedBeatPulse) * 0.12;
  const beatPulse = smoothedBeatPulse;
  context.lineWidth = Math.max(1.5, size * 0.004);
  context.fillStyle = '#b9f36b';
  const elapsed = Math.min(50, Math.max(0, time - lastOrbitTime));
  lastOrbitTime = time;
  const orbitSpeed = 0.00028 + beatPulse * 0.0008;
  orbitPhase += elapsed * orbitSpeed;
  const orbitRotation = orbitPhase;
  const orbitColors = [[255, 95, 85], [185, 243, 107], [193, 166, 255]];
  const bassPerspective = 1 + beatPulse * 0.14;
  const bassOrbitScale = 1 + beatPulse * 0.45;
  const orbitRadii = [
    [Math.min(size * 0.34, size * 0.27 * bassOrbitScale), Math.min(size * 0.23, size * 0.15 * bassOrbitScale)],
    [Math.min(size * 0.42, size * 0.335 * bassOrbitScale), Math.min(size * 0.29, size * 0.195 * bassOrbitScale)],
    [Math.min(size * 0.49, size * 0.4 * bassOrbitScale), Math.min(size * 0.35, size * 0.24 * bassOrbitScale)],
  ];
  for (let index = 0; index < 3; index += 1) {
    context.save();
    context.translate(centerX, centerY);
    context.rotate(orbitRotation + index * (Math.PI / 3));
    const response = smoothedOrbitResponses[index];
    context.globalAlpha = 0.38 + response * 0.5;
    const colorAmount = Math.min(1, Math.max(0, (response - 0.25) / 0.75));
    const peakColor = orbitColors[index];
    const red = Math.round(103 + (peakColor[0] - 103) * colorAmount);
    const green = Math.round(119 + (peakColor[1] - 119) * colorAmount);
    const blue = Math.round(151 + (peakColor[2] - 151) * colorAmount);
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
    const angle = orbitPhase * (1.7 + index * 0.1) + index * 2.1;
    const orientation = orbitRotation + index * (Math.PI / 3);
    const depth = (Math.sin(angle) + 1) * 0.5;
    const depthScale = bassPerspective * (0.72 + depth * beatPulse * 0.5);
    const localX = Math.cos(angle) * orbitRadii[index][0];
    const localY = Math.sin(angle) * orbitRadii[index][1];
    const x = centerX + localX * Math.cos(orientation) - localY * Math.sin(orientation);
    const y = centerY + localX * Math.sin(orientation) + localY * Math.cos(orientation);
    context.beginPath();
    context.fillStyle = colors[index];
    context.globalAlpha = 0.45 + depth * 0.55;
    context.shadowBlur = 8 + depth * beatPulse * 16;
    context.shadowColor = colors[index];
    context.arc(x, y, Math.max(3, Math.min(width, height) * 0.012 * depthScale), 0, Math.PI * 2);
    context.fill();
    context.shadowBlur = 0;
  });
  context.globalAlpha = 1;
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
    const level = smoothedLevels.length ? spectrumLevel * 1.8 : fallback;
    const normalizedLevel = Math.min(1, Math.max(0, level));
    const adjustedLevel = Math.max(0.08, normalizedLevel) * (props.active ? 1 : 0.45);
    const barHeight = Math.min(height * 0.76, height * (0.12 + adjustedLevel * 0.62));
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
  else drawMascot(context, width, height, time);
  animationFrame = requestAnimationFrame(render);
}

watch(() => props.kind, () => { startedAt = performance.now(); });

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
  <canvas ref="canvas" class="visualizer-canvas" aria-hidden="true" />
</template>

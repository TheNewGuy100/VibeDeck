<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { createMascotAudioResponseState, updateMascotAudioResponse } from './mascot-audio-response';
import { DEFAULT_SERPENT_RESOLUTION, shouldAnimateVisualizer } from './visualizer-performance';
import type { SerpentLight, SerpentSnake } from './serpents-layout';
import { loadSharedSerpentsPreferences, serpentsLights as lights, serpentsSnakes as snakes } from '../services/serpents-preferences';
// The downloaded prototype is intentionally isolated so it can be compared with the previous renderer.
// @ts-ignore The prototype is plain JavaScript and exposes its own factory API.
import { createSnakeVisualizer } from './snake-visualizer-core.js';

const props = withDefaults(defineProps<{
  active?: boolean;
  hasTrack?: boolean;
  visible?: boolean;
  compact?: boolean;
  levels?: number[];
  density?: number;
  detail?: number;
  glow?: number;
  resolution?: number;
  sensitivity?: number;
  smoothing?: number;
  intensity?: number;
  motion?: number;
}>(), { active: false, hasTrack: true, visible: true, compact: false, levels: undefined, density: 1, detail: 1, glow: 1, resolution: DEFAULT_SERPENT_RESOLUTION, sensitivity: 1, smoothing: 0.5, intensity: 1, motion: 1 });

const host = ref<HTMLDivElement>();
let visualizer: { updateAudioAnalysis: (data: Record<string, number>) => void; setRunning: (running: boolean) => void; setNormalMapRepeat: (repeatX: number, repeatY: number) => void; setNormalMapTransform: (rotation: number, movement: number) => void; setGeometryShape: (curveAngle: number, centerGap: number, amplitude: number, thickness: number) => void; setSnakeLayout: (layout: SerpentSnake[]) => void; setUserLights: (layout: SerpentLight[]) => void; getStats: () => Record<string, unknown>; dispose: () => void } | undefined;
const normalMapRepeatX = ref(0.75);
const normalMapRepeatY = ref(5);
const normalMapRotation = ref(91);
const normalMapMovement = ref(6);
const curveAngle = ref(24);
const centerGap = ref(4.8);
const curveAmplitude = ref(0.68);
const thickness = ref(1.30);
const audioResponseState = createMascotAudioResponseState();
let reducedMotionQuery: MediaQueryList | undefined;
let handleReducedMotionChange: ((event: MediaQueryListEvent) => void) | undefined;

function shouldRun() {
  return shouldAnimateVisualizer(props.active, props.hasTrack, Boolean(reducedMotionQuery?.matches), props.visible);
}

function average(values: number[], start: number, end: number) {
  const slice = values.slice(start, Math.max(start + 1, end));
  return slice.length ? slice.reduce((sum, value) => sum + value, 0) / slice.length : 0;
}

function updateAnalysis() {
  const levels = props.active ? (props.levels ?? []) : [];
  const bassEnd = Math.max(4, Math.floor(levels.length * 0.25));
  const midsEnd = Math.max(bassEnd + 1, Math.floor(levels.length * 0.68));
  const bass = average(levels, 0, bassEnd);
  const gain = props.sensitivity * props.intensity;
  const response = updateMascotAudioResponse(audioResponseState, levels, performance.now(), {
    bassMinHz: 20,
    bassMaxHz: 250,
    voiceMinHz: 300,
    voiceMaxHz: 3500,
    energyMinHz: 4000,
    energyMaxHz: 16000,
  });
  visualizer?.updateAudioAnalysis({
    subBass: Math.min(1, average(levels, 0, Math.max(2, Math.floor(levels.length * 0.1))) * gain),
    bass: Math.min(1, bass * gain),
    mids: Math.min(1, average(levels, bassEnd, midsEnd) * gain),
    highs: Math.min(1, average(levels, midsEnd, levels.length) * gain),
    energy: Math.min(1, average(levels, 0, levels.length) * gain),
    beat: Math.min(1, response.beat * gain),
  });
}

function create() {
  if (!host.value) return;
  const scale = props.compact ? 0.65 : 1;
  const created = createSnakeVisualizer(host.value, {
    render: { maxPixelRatio: Math.min(1.25, Math.max(0.75, props.resolution)) },
    geometry: { tubularSegments: Math.round(80 * scale), radialSegments: props.compact ? 8 : 10 },
    layers: [
      { z: 2, count: Math.max(2, Math.round(4 * props.density)), radius: 0.44 * scale, scale: 1.05 },
      { z: -0.5, count: Math.max(2, Math.round(6 * props.density)), radius: 0.38 * scale, scale: 1 },
      { z: -3, count: Math.max(2, Math.round(8 * props.density)), radius: 0.32 * scale, scale: 0.95 },
      { z: -6, count: Math.max(2, Math.round(9 * props.density)), radius: 0.26 * scale, scale: 0.9 },
    ],
    bloom: { strength: 0.38 * props.glow, radius: 0.46, threshold: 0.34 },
  });
  visualizer = created;
  created.setRunning(shouldRun());
  created.setNormalMapRepeat(normalMapRepeatX.value, normalMapRepeatY.value);
  created.setNormalMapTransform(normalMapRotation.value, normalMapMovement.value * props.motion);
  created.setGeometryShape(curveAngle.value, centerGap.value, curveAmplitude.value, thickness.value);
  created.setSnakeLayout(snakes.value);
  created.setUserLights(lights.value);
  window.setTimeout(() => console.info('[VibeDeck][Serpents3D] stats', created.getStats()), 500);
  updateAnalysis();
}

watch(() => props.levels, updateAnalysis);
watch([() => props.active, () => props.hasTrack, () => props.visible], () => {
  updateAnalysis();
  visualizer?.setRunning(shouldRun());
});
watch([normalMapRepeatX, normalMapRepeatY], ([repeatX, repeatY]) => visualizer?.setNormalMapRepeat(repeatX, repeatY));
watch([normalMapRotation, normalMapMovement, () => props.motion], ([rotation, movement, motion]) => visualizer?.setNormalMapTransform(rotation, movement * motion));
watch([curveAngle, centerGap, curveAmplitude, thickness], ([angle, gap, amplitude, nextThickness]) => visualizer?.setGeometryShape(angle, gap, amplitude, nextThickness));
watch(snakes, (value) => visualizer?.setSnakeLayout(value), { deep: true });
watch(lights, (value) => visualizer?.setUserLights(value), { deep: true });
onMounted(() => {
  reducedMotionQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
  handleReducedMotionChange = () => visualizer?.setRunning(shouldRun());
  reducedMotionQuery.addEventListener?.('change', handleReducedMotionChange);
  void loadSharedSerpentsPreferences();
  create();
});
onBeforeUnmount(() => {
  if (reducedMotionQuery && handleReducedMotionChange) reducedMotionQuery.removeEventListener?.('change', handleReducedMotionChange);
  visualizer?.dispose();
});
</script>

<template>
  <div class="serpents-3d-shell">
    <div ref="host" class="serpents-3d" aria-hidden="true" />
  </div>
</template>

<style scoped>
.serpents-3d-shell,
.serpents-3d {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 260px;
}

:global(.serpents-3d-shell.playlist-visualizer) {
  position: fixed !important;
  top: 72px;
  right: 0;
  bottom: 92px;
  left: 244px;
  z-index: 0;
  width: auto;
  height: auto;
  min-height: 0;
  margin: 0;
  opacity: .18;
  pointer-events: none;
}

:global(.serpents-3d-shell.playlist-visualizer) .serpents-3d {
  min-height: 0;
}
</style>

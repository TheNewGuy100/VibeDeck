<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
// The downloaded prototype is intentionally isolated so it can be compared with the previous renderer.
// @ts-ignore The prototype is plain JavaScript and exposes its own factory API.
import { createSnakeVisualizer } from './snake-visualizer-core.js';

const props = withDefaults(defineProps<{
  active?: boolean;
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
}>(), { active: true, compact: false, levels: undefined, density: 1, detail: 1, glow: 1, resolution: 1, sensitivity: 1, smoothing: 0.5, intensity: 1, motion: 1 });

const host = ref<HTMLDivElement>();
let visualizer: { updateAudioAnalysis: (data: Record<string, number>) => void; setRunning: (running: boolean) => void; setNormalMapRepeat: (repeatX: number, repeatY: number) => void; setNormalMapTransform: (rotation: number, movement: number) => void; setGeometryShape: (curveAngle: number, centerGap: number, amplitude: number, thickness: number) => void; getStats: () => Record<string, unknown>; dispose: () => void } | undefined;
const normalMapRepeatX = ref(0.75);
const normalMapRepeatY = ref(5);
const normalMapRotation = ref(91);
const normalMapMovement = ref(6);
const curveAngle = ref(24);
const centerGap = ref(4.8);
const curveAmplitude = ref(0.68);
const thickness = ref(1.30);

function average(values: number[], start: number, end: number) {
  const slice = values.slice(start, Math.max(start + 1, end));
  return slice.length ? slice.reduce((sum, value) => sum + value, 0) / slice.length : 0;
}

function updateAnalysis() {
  const levels = props.levels ?? [];
  const bassEnd = Math.max(4, Math.floor(levels.length * 0.25));
  const midsEnd = Math.max(bassEnd + 1, Math.floor(levels.length * 0.68));
  const bass = average(levels, 0, bassEnd);
  const gain = props.sensitivity * props.intensity;
  visualizer?.updateAudioAnalysis({
    subBass: Math.min(1, average(levels, 0, Math.max(2, Math.floor(levels.length * 0.1))) * gain),
    bass: Math.min(1, bass * gain),
    mids: Math.min(1, average(levels, bassEnd, midsEnd) * gain),
    highs: Math.min(1, average(levels, midsEnd, levels.length) * gain),
    energy: Math.min(1, average(levels, 0, levels.length) * gain),
    beat: bass,
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
  created.setRunning(props.active);
  created.setNormalMapRepeat(normalMapRepeatX.value, normalMapRepeatY.value);
  created.setNormalMapTransform(normalMapRotation.value, normalMapMovement.value * props.motion);
  created.setGeometryShape(curveAngle.value, centerGap.value, curveAmplitude.value, thickness.value);
  window.setTimeout(() => console.info('[VibeDeck][Serpents3D] stats', created.getStats()), 500);
  updateAnalysis();
}

watch(() => props.levels, updateAnalysis);
watch(() => props.active, (active) => visualizer?.setRunning(active));
watch([normalMapRepeatX, normalMapRepeatY], ([repeatX, repeatY]) => visualizer?.setNormalMapRepeat(repeatX, repeatY));
watch([normalMapRotation, normalMapMovement, () => props.motion], ([rotation, movement, motion]) => visualizer?.setNormalMapTransform(rotation, movement * motion));
watch([curveAngle, centerGap, curveAmplitude, thickness], ([angle, gap, amplitude, nextThickness]) => visualizer?.setGeometryShape(angle, gap, amplitude, nextThickness));

onMounted(create);
onBeforeUnmount(() => visualizer?.dispose());
</script>

<template>
  <div class="serpents-3d-shell">
    <div ref="host" class="serpents-3d" aria-hidden="true" />
    <details class="normal-map-control" open>
      <summary>Controles do visualizer</summary>
      <!-- Layout dos tubos agora e deterministico; estes controles so calibram o visual. -->
      <label>
        <span>Repetição X <input class="value-input" v-model.number="normalMapRepeatX" type="number" step="0.25" aria-label="Valor da repetição X" /></span>
        <input v-model.number="normalMapRepeatX" type="range" min="0.25" max="16" step="0.25" aria-label="Repetição horizontal do normal map" />
      </label>
      <label>
        <span>Repetição Y <input class="value-input" v-model.number="normalMapRepeatY" type="number" step="0.25" aria-label="Valor da repetição Y" /></span>
        <input v-model.number="normalMapRepeatY" type="range" min="0.25" max="16" step="0.25" aria-label="Repetição vertical do normal map" />
      </label>
      <label>
        <span>Rotação <input class="value-input" v-model.number="normalMapRotation" type="number" step="1" aria-label="Valor da rotação" />°</span>
        <input v-model.number="normalMapRotation" type="range" min="-180" max="180" step="1" aria-label="Rotação do normal map" />
      </label>
      <label>
        <span>Movimento <input class="value-input" v-model.number="normalMapMovement" type="number" step="0.05" aria-label="Valor do movimento" /></span>
        <input v-model.number="normalMapMovement" type="range" min="-6" max="6" step="0.05" aria-label="Movimento do normal map" />
      </label>
      <label>
        <span>Ângulo da curva <input class="value-input" v-model.number="curveAngle" type="number" step="1" aria-label="Valor do ângulo da curva" />°</span>
        <input v-model.number="curveAngle" type="range" min="-40" max="40" step="1" aria-label="Ângulo da curva das cobras" />
      </label>
      <label>
        <span>Espaço central <input class="value-input" v-model.number="centerGap" type="number" step="0.1" aria-label="Valor do espaço central" /></span>
        <input v-model.number="centerGap" type="range" min="0" max="5" step="0.1" aria-label="Espaço central do arco" />
      </label>
      <label>
        <span>Amplitude <input class="value-input" v-model.number="curveAmplitude" type="number" step="0.05" aria-label="Valor da amplitude" /></span>
        <input v-model.number="curveAmplitude" type="range" min="0" max="1.5" step="0.05" aria-label="Amplitude das curvas" />
      </label>
      <label>
        <span>Grossura <input class="value-input" v-model.number="thickness" type="number" step="0.05" aria-label="Valor da grossura" /></span>
        <input v-model.number="thickness" type="range" min="0.4" max="2.5" step="0.05" aria-label="Grossura das curvas" />
      </label>
    </details>
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

.normal-map-control {
  position: absolute;
  top: 14px;
  right: 14px;
  z-index: 3;
  display: grid;
  gap: 6px;
  width: 150px;
  padding: 9px 11px;
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 10px;
  color: rgba(255, 255, 255, 0.82);
  background: rgba(5, 8, 12, 0.72);
  backdrop-filter: blur(10px);
  font-size: 11px;
  letter-spacing: 0.04em;
  pointer-events: auto;
}

.normal-map-control label {
  display: grid;
  gap: 6px;
}

.normal-map-control summary {
  margin-bottom: 6px;
  color: #b9f36b;
  cursor: pointer;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.05em;
  list-style: none;
}

.normal-map-control summary::-webkit-details-marker {
  display: none;
}

:global(.playlist-visualizer) .normal-map-control,
:global(body:not(.developer-mode)) .visualizer-drawer .normal-map-control {
  display: none;
}

.normal-map-control span {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.normal-map-control strong {
  color: #ff5a5a;
}

.normal-map-control input {
  width: 100%;
  accent-color: #ff3939;
}

.normal-map-control .value-input {
  width: 48px;
  padding: 1px 3px;
  border: 0;
  border-bottom: 1px solid rgba(255, 90, 90, 0.7);
  border-radius: 0;
  color: #ff5a5a;
  background: transparent;
  font: inherit;
  font-weight: 700;
  text-align: right;
}
</style>

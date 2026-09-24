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
}>(), { active: true, compact: false, levels: undefined, density: 1, detail: 1, glow: 1, resolution: 1 });

const host = ref<HTMLDivElement>();
let visualizer: { updateAudioAnalysis: (data: Record<string, number>) => void; setRunning: (running: boolean) => void; setNormalMapRepeat: (repeatX: number, repeatY: number) => void; setNormalMapTransform: (rotation: number, movement: number) => void; setGeometryShape: (curveAngle: number, centerGap: number, endpointCenter: number, amplitude: number, minEndpointDistance: number, thickness: number, curvesPerEdge: number) => void; getStats: () => Record<string, unknown>; dispose: () => void } | undefined;
const normalMapRepeatX = ref(0.75);
const normalMapRepeatY = ref(5);
const normalMapRotation = ref(91);
const normalMapMovement = ref(6);
const curveAngle = ref(30);
const centerGap = ref(5);
const endpointCenter = ref(-0.4);
const curveAmplitude = ref(0.7);
const minEndpointDistance = ref(22.25);
const thickness = ref(1.8);
const curvesPerEdge = ref(2);

function average(values: number[], start: number, end: number) {
  const slice = values.slice(start, Math.max(start + 1, end));
  return slice.length ? slice.reduce((sum, value) => sum + value, 0) / slice.length : 0;
}

function updateAnalysis() {
  const levels = props.levels ?? [];
  const bassEnd = Math.max(4, Math.floor(levels.length * 0.25));
  const midsEnd = Math.max(bassEnd + 1, Math.floor(levels.length * 0.68));
  const bass = average(levels, 0, bassEnd);
  visualizer?.updateAudioAnalysis({
    subBass: average(levels, 0, Math.max(2, Math.floor(levels.length * 0.1))),
    bass,
    mids: average(levels, bassEnd, midsEnd),
    highs: average(levels, midsEnd, levels.length),
    energy: average(levels, 0, levels.length),
    beat: bass,
  });
}

function create() {
  if (!host.value) return;
  const scale = props.compact ? 0.65 : 1;
  const created = createSnakeVisualizer(host.value, {
    render: { maxPixelRatio: Math.min(2.5, props.resolution) },
    geometry: { tubularSegments: Math.round(80 * scale), radialSegments: props.compact ? 8 : 10 },
    layers: [
      { z: 2, count: Math.max(2, Math.round(4 * props.density)), radius: 0.44 * scale, scale: 1.05 },
      { z: -0.5, count: Math.max(2, Math.round(6 * props.density)), radius: 0.38 * scale, scale: 1 },
      { z: -3, count: Math.max(2, Math.round(8 * props.density)), radius: 0.32 * scale, scale: 0.95 },
      { z: -6, count: Math.max(2, Math.round(9 * props.density)), radius: 0.26 * scale, scale: 0.9 },
    ],
    bloom: { strength: 0.45 * props.glow, radius: 0.48, threshold: 0.3 },
  });
  visualizer = created;
  created.setRunning(props.active);
  created.setNormalMapRepeat(normalMapRepeatX.value, normalMapRepeatY.value);
  created.setNormalMapTransform(normalMapRotation.value, normalMapMovement.value);
  created.setGeometryShape(curveAngle.value, centerGap.value, endpointCenter.value, curveAmplitude.value, minEndpointDistance.value, thickness.value, curvesPerEdge.value);
  window.setTimeout(() => console.info('[VibeDeck][SnakePrototype] stats', created.getStats()), 500);
  updateAnalysis();
}

watch(() => props.levels, updateAnalysis);
watch(() => props.active, (active) => visualizer?.setRunning(active));
watch([normalMapRepeatX, normalMapRepeatY], ([repeatX, repeatY]) => visualizer?.setNormalMapRepeat(repeatX, repeatY));
watch([normalMapRotation, normalMapMovement], ([rotation, movement]) => visualizer?.setNormalMapTransform(rotation, movement));
watch([curveAngle, centerGap, endpointCenter, curveAmplitude, minEndpointDistance, thickness, curvesPerEdge], ([angle, gap, proximity, amplitude, minDistance, nextThickness, nextCurvesPerEdge]) => visualizer?.setGeometryShape(angle, gap, proximity, amplitude, minDistance, nextThickness, nextCurvesPerEdge));

onMounted(create);
onBeforeUnmount(() => visualizer?.dispose());
</script>

<template>
  <div class="serpents-3d-shell">
    <div ref="host" class="serpents-3d" aria-hidden="true" />
    <div class="normal-map-control">
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
        <span>Pontas ao centro <input class="value-input" v-model.number="endpointCenter" type="number" step="0.05" aria-label="Valor da proximidade das pontas" /></span>
        <input v-model.number="endpointCenter" type="range" min="-4" max="1" step="0.05" aria-label="Proximidade das pontas ao centro" />
      </label>
      <label>
        <span>Amplitude <input class="value-input" v-model.number="curveAmplitude" type="number" step="0.05" aria-label="Valor da amplitude" /></span>
        <input v-model.number="curveAmplitude" type="range" min="0" max="1.5" step="0.05" aria-label="Amplitude das curvas" />
      </label>
      <label>
        <span>Distância mínima <input class="value-input" v-model.number="minEndpointDistance" type="number" step="0.25" aria-label="Valor da distância mínima" /></span>
        <input v-model.number="minEndpointDistance" type="range" min="0" max="36" step="0.25" aria-label="Distância mínima entre as pontas" />
      </label>
      <label>
        <span>Grossura <input class="value-input" v-model.number="thickness" type="number" step="0.05" aria-label="Valor da grossura" /></span>
        <input v-model.number="thickness" type="range" min="0.4" max="2.5" step="0.05" aria-label="Grossura das curvas" />
      </label>
      <label>
        <span>Curvas por canto <input class="value-input" v-model.number="curvesPerEdge" type="number" min="1" max="8" step="1" aria-label="Número de curvas por canto" /></span>
        <input v-model.number="curvesPerEdge" type="range" min="1" max="8" step="1" aria-label="Curvas por canto" />
      </label>
    </div>
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

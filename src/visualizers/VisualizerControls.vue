<script setup lang="ts">
import { onMounted, ref, watch } from 'vue';
import FrequencyRangeControl from '../components/FrequencyRangeControl.vue';
import { addLight, addSnake, MAX_LIGHTS, MAX_SNAKES, MIN_LIGHTS, MIN_SNAKES, removeLight, removeSnake } from './serpents-layout';
import { loadSharedSerpentsPreferences, serpentsLights, serpentsSnakes } from '../services/serpents-preferences';
import { createVisualizerPreferenceController, DEFAULT_VISUALIZER_CONTROLS_OPEN } from '../services/visualizer-preferences';
const props = withDefaults(defineProps<{
  visible?: boolean;
  sensitivity: number;
  smoothing: number;
  intensity: number;
  motion: number;
  kind: 'mascot' | 'bars' | 'serpents' | 'spectrogram';
  bassSensitivity: number;
  bassDistance: number;
  bassMinHz: number;
  bassMaxHz: number;
  energyMinHz: number;
  energyMaxHz: number;
  voiceMinHz: number;
  voiceMaxHz: number;
  motionMinHz: number;
  motionMaxHz: number;
}>(), { visible: false });

const emit = defineEmits<{
  'update:sensitivity': [value: number];
  'update:smoothing': [value: number];
  'update:intensity': [value: number];
  'update:motion': [value: number];
  'update:bassSensitivity': [value: number];
  'update:bassDistance': [value: number];
  'update:bassMinHz': [value: number];
  'update:bassMaxHz': [value: number];
  'update:energyMinHz': [value: number];
  'update:energyMaxHz': [value: number];
  'update:voiceMinHz': [value: number];
  'update:voiceMaxHz': [value: number];
  'update:motionMinHz': [value: number];
  'update:motionMaxHz': [value: number];
}>();
const spectrogramOpen = ref(false);
const controlsOpen = ref(DEFAULT_VISUALIZER_CONTROLS_OPEN);
const preferenceController = createVisualizerPreferenceController();
const preserveSpectrogramState = ref(false);
function clampValue(value: number, min: number, max: number, fallback: number) {
  return Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
}

function readValue(event: Event, min: number, max: number, fallback: number) {
  return clampValue(Number((event.target as HTMLInputElement).value), min, max, fallback);
}

function readFrequency(value: number, min: number, max: number, fallback: number) {
  return clampValue(value, min, max, fallback);
}

function openSpectrogram() {
  spectrogramOpen.value = !spectrogramOpen.value;
  preserveSpectrogramState.value = true;
  window.dispatchEvent(new CustomEvent('vibedeck:select-visualizer', { detail: 'spectrogram-toggle' }));
}
watch(() => props.kind, () => {
  if (preserveSpectrogramState.value) {
    preserveSpectrogramState.value = false;
    return;
  }
  spectrogramOpen.value = false;
});

onMounted(() => preferenceController.load((open) => { controlsOpen.value = open; }));
watch(() => props.kind, kind => { if (kind === 'serpents') void loadSharedSerpentsPreferences(); }, { immediate: true });

function onControlsToggle(event: Event) {
  const open = (event.currentTarget as HTMLDetailsElement).open;
  if (controlsOpen.value === open) return;
  controlsOpen.value = open;
  void preferenceController.set(open);
}
</script>

<template>
  <details v-if="visible" class="visualizer-general-controls" :open="controlsOpen" @toggle="onControlsToggle">
    <summary>Controles gerais</summary>
     <div class="controls-body"><div>
       <button type="button" class="spectrogram-button" :aria-pressed="spectrogramOpen" @click="openSpectrogram">{{ spectrogramOpen ? 'Voltar para visualizer' : 'Abrir espectrograma' }}</button>
       <label><span>Sensibilidade <input class="value-input" :value="sensitivity" type="number" min="0" max="3" step="0.05" aria-label="Sensibilidade em valor numérico" @input="emit('update:sensitivity', readValue($event, 0, 3, sensitivity))" /></span><input :value="sensitivity" type="range" min="0" max="3" step="0.05" aria-label="Sensibilidade" @input="emit('update:sensitivity', readValue($event, 0, 3, sensitivity))" /></label>
       <label><span>Suavização <input class="value-input" :value="smoothing" type="number" min="0" max="1" step="0.05" aria-label="Suavização em valor numérico" @input="emit('update:smoothing', readValue($event, 0, 1, smoothing))" /></span><input :value="smoothing" type="range" min="0" max="1" step="0.05" aria-label="Suavização" @input="emit('update:smoothing', readValue($event, 0, 1, smoothing))" /></label>
       <label><span>Intensidade <input class="value-input" :value="intensity" type="number" min="0" max="2" step="0.05" aria-label="Intensidade em valor numérico" @input="emit('update:intensity', readValue($event, 0, 2, intensity))" /></span><input :value="intensity" type="range" min="0" max="2" step="0.05" aria-label="Intensidade" @input="emit('update:intensity', readValue($event, 0, 2, intensity))" /></label>
       <label><span>Movimento <input class="value-input" :value="motion" type="number" min="0" max="2" step="0.05" aria-label="Movimento em valor numérico" @input="emit('update:motion', readValue($event, 0, 2, motion))" /></span><input :value="motion" type="range" min="0" max="2" step="0.05" aria-label="Movimento" @input="emit('update:motion', readValue($event, 0, 2, motion))" /></label>
       <template v-if="kind === 'mascot'">
        <p class="section-label">Mascote</p>
         <label><span>Força de aproximação <input class="value-input" :value="bassSensitivity" type="number" min="0" max="3" step="0.05" aria-label="Força de aproximação em valor numérico" @input="emit('update:bassSensitivity', readValue($event, 0, 3, bassSensitivity))" /></span><input :value="bassSensitivity" type="range" min="0" max="3" step="0.05" aria-label="Força de aproximação" @input="emit('update:bassSensitivity', readValue($event, 0, 3, bassSensitivity))" /></label>
         <label><span>Distância do movimento <input class="value-input" :value="bassDistance" type="number" min="0" max="3" step="0.05" aria-label="Distância do movimento em valor numérico" @input="emit('update:bassDistance', readValue($event, 0, 3, bassDistance))" /></span><input :value="bassDistance" type="range" min="0" max="3" step="0.05" aria-label="Distância do movimento" @input="emit('update:bassDistance', readValue($event, 0, 3, bassDistance))" /></label>
         <FrequencyRangeControl label="Bass vermelho" :min-value="bassMinHz" :max-value="bassMaxHz" @update:min-value="emit('update:bassMinHz', readFrequency($event, 20, bassMaxHz - 1, bassMinHz))" @update:max-value="emit('update:bassMaxHz', readFrequency($event, bassMinHz + 1, 20000, bassMaxHz))" />
         <FrequencyRangeControl label="Energia verde" :min-value="energyMinHz" :max-value="energyMaxHz" @update:min-value="emit('update:energyMinHz', readFrequency($event, 20, energyMaxHz - 1, energyMinHz))" @update:max-value="emit('update:energyMaxHz', readFrequency($event, energyMinHz + 1, 20000, energyMaxHz))" />
         <FrequencyRangeControl label="Voz roxa" :min-value="voiceMinHz" :max-value="voiceMaxHz" @update:min-value="emit('update:voiceMinHz', readFrequency($event, 20, voiceMaxHz - 1, voiceMinHz))" @update:max-value="emit('update:voiceMaxHz', readFrequency($event, voiceMinHz + 1, 20000, voiceMaxHz))" />
         <FrequencyRangeControl label="Movimento do Mascote" :min-value="motionMinHz" :max-value="motionMaxHz" @update:min-value="emit('update:motionMinHz', readFrequency($event, 20, motionMaxHz - 1, motionMinHz))" @update:max-value="emit('update:motionMaxHz', readFrequency($event, motionMinHz + 1, 20000, motionMaxHz))" />
       </template>
       <template v-if="kind === 'serpents'">
         <p class="section-label">Serpentes · Cobras ({{ serpentsSnakes.length }}/{{ MAX_SNAKES }})</p>
         <button class="control-action" type="button" :disabled="serpentsSnakes.length >= MAX_SNAKES" @click="serpentsSnakes = addSnake(serpentsSnakes)">Adicionar cobra</button>
         <fieldset v-for="(snake, index) in serpentsSnakes" :key="snake.id" class="serpent-control-group"><legend>Cobra {{ index + 1 }}</legend>
           <label><span>Faixa e direção</span><select v-model="snake.lane" :aria-label="`Faixa e direção da cobra ${index + 1}`"><option value="left">Esquerda · sobe</option><option value="right">Direita · sobe</option><option value="center">Centro · desce</option></select></label>
           <label><span>Posição X</span><input v-model.number="snake.x" type="range" min="-9" max="9" step="0.25" :aria-label="`Posição X da cobra ${index + 1}`" /></label>
           <label><span>Profundidade</span><input v-model.number="snake.z" type="range" min="-6" max="3" step="0.25" :aria-label="`Profundidade da cobra ${index + 1}`" /></label>
           <label><span>Escala</span><input v-model.number="snake.scale" type="range" min="0.5" max="1.4" step="0.05" :aria-label="`Escala da cobra ${index + 1}`" /></label>
           <label><span>Fase</span><input v-model.number="snake.phase" type="range" min="0" max="6.28" step="0.1" :aria-label="`Fase da cobra ${index + 1}`" /></label>
           <button class="control-action" type="button" :disabled="serpentsSnakes.length <= MIN_SNAKES" :aria-label="`Remover cobra ${index + 1}`" @click="serpentsSnakes = removeSnake(serpentsSnakes, snake.id)">Remover cobra</button>
         </fieldset>
         <p class="section-label">Serpentes · Iluminação ({{ serpentsLights.length }}/{{ MAX_LIGHTS }})</p>
         <button class="control-action" type="button" :disabled="serpentsLights.length >= MAX_LIGHTS" @click="serpentsLights = addLight(serpentsLights)">Adicionar luz-chave</button>
         <fieldset v-for="(light, index) in serpentsLights" :key="light.id" class="serpent-control-group"><legend>Luz {{ index + 1 }}</legend>
           <label><span>Posição X</span><input v-model.number="light.x" type="range" min="-12" max="12" step="0.5" :aria-label="`Posição X da luz ${index + 1}`" /></label>
           <label><span>Posição Y</span><input v-model.number="light.y" type="range" min="-9" max="9" step="0.5" :aria-label="`Posição Y da luz ${index + 1}`" /></label>
           <label><span>Profundidade Z</span><input v-model.number="light.z" type="range" min="-10" max="14" step="0.5" :aria-label="`Profundidade Z da luz ${index + 1}`" /></label>
           <button class="control-action" type="button" :disabled="serpentsLights.length <= MIN_LIGHTS" :aria-label="`Remover luz ${index + 1}`" @click="serpentsLights = removeLight(serpentsLights, light.id)">Remover luz</button>
         </fieldset>
       </template>
    </div></div>
  </details>
</template>

<style scoped>
.visualizer-general-controls { position: absolute; top: 14px; right: 14px; z-index: 4; width: 148px; height: 38px; max-height: calc(100% - 28px); padding: 10px 12px; overflow: hidden; border: 1px solid rgba(255, 255, 255, .14); border-radius: 10px; color: rgba(255, 255, 255, .82); background: rgba(5, 8, 12, .78); backdrop-filter: blur(10px); font-size: 11px; transition: width .24s ease, height .24s ease, background .24s ease; scrollbar-width: thin; scrollbar-color: rgba(185, 243, 107, .65) transparent; }
.visualizer-general-controls[open] { width: 330px; height: calc(100% - 28px); overflow-y: auto; }
.visualizer-general-controls[open]:has(.serpent-control-group) { width: min(380px, calc(100vw - 28px)); }
.serpent-control-group { display: grid; gap: 5px; min-width: 0; margin: 10px 0; padding: 8px; border: 1px solid rgba(255,255,255,.16); border-radius: 7px; }
.serpent-control-group legend { color: #b9f36b; font-weight: 700; }
.serpent-control-group select { width: 100%; color: inherit; background: #111820; border: 1px solid rgba(255,255,255,.2); padding: 5px; }
.control-action { width: 100%; margin-top: 6px; padding: 6px; color: inherit; background: rgba(185,243,107,.08); border: 1px solid rgba(185,243,107,.35); border-radius: 5px; cursor: pointer; }
.control-action:disabled { opacity: .45; cursor: not-allowed; }
.serpent-control-group :focus-visible, .control-action:focus-visible { outline: 2px solid #b9f36b; outline-offset: 2px; }
.visualizer-general-controls summary { margin-bottom: 0; color: #b9f36b; cursor: pointer; font-weight: 700; letter-spacing: .05em; list-style: none; white-space: nowrap; }
.visualizer-general-controls summary::-webkit-details-marker { display: none; }
.visualizer-general-controls label { display: grid; gap: 5px; margin-top: 8px; }
.controls-body { display: grid; grid-template-rows: 0fr; overflow: hidden; transition: grid-template-rows .24s ease, opacity .2s ease; opacity: 0; }
.visualizer-general-controls[open] .controls-body { grid-template-rows: 1fr; opacity: 1; }
.controls-body > div { min-height: 0; }
.section-label { margin: 12px 0 4px; color: #b9f36b; font-size: 10px; font-weight: 700; letter-spacing: .05em; }
.visualizer-general-controls span { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.visualizer-general-controls input[type='range'] { width: 100%; accent-color: #b9f36b; }
.value-input { width: 48px; padding: 1px 3px; border: 0; border-bottom: 1px solid rgba(185, 243, 107, .65); color: #b9f36b; background: transparent; font: inherit; font-weight: 700; text-align: right; }
.spectrogram-button { width: 100%; margin: 10px 0 14px; padding: 6px 8px; position: relative; border: 1px solid rgba(185, 243, 107, .5); border-radius: 6px; color: #b9f36b; background: rgba(185, 243, 107, .08); cursor: pointer; font: inherit; font-weight: 700; }
.spectrogram-button::after { content: ''; position: absolute; right: 0; bottom: -9px; left: 0; border-bottom: 1px solid rgba(255, 255, 255, .14); }
.visualizer-general-controls summary:focus-visible, .visualizer-general-controls input:focus-visible, .spectrogram-button:focus-visible { outline: 2px solid #b9f36b; outline-offset: 2px; }
</style>

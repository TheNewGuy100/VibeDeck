<script setup lang="ts">
import { ref, watch } from 'vue';
import FrequencyRangeControl from './FrequencyRangeControl.vue';
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
const preserveSpectrogramState = ref(false);
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
</script>

<template>
  <details v-if="visible" class="visualizer-general-controls" open>
    <summary>Controles gerais</summary>
     <div class="controls-body"><div>
       <button class="spectrogram-button" @click="openSpectrogram">{{ spectrogramOpen ? 'Voltar para visualizer' : 'Abrir espectrograma' }}</button>
      <label><span>Sensibilidade <input class="value-input" :value="sensitivity" type="number" min="0" max="3" step="0.05" @input="emit('update:sensitivity', Number(($event.target as HTMLInputElement).value))" /></span><input :value="sensitivity" type="range" min="0" max="3" step="0.05" @input="emit('update:sensitivity', Number(($event.target as HTMLInputElement).value))" /></label>
      <label><span>Suavização <input class="value-input" :value="smoothing" type="number" min="0" max="1" step="0.05" @input="emit('update:smoothing', Number(($event.target as HTMLInputElement).value))" /></span><input :value="smoothing" type="range" min="0" max="1" step="0.05" @input="emit('update:smoothing', Number(($event.target as HTMLInputElement).value))" /></label>
      <label><span>Intensidade <input class="value-input" :value="intensity" type="number" min="0" max="2" step="0.05" @input="emit('update:intensity', Number(($event.target as HTMLInputElement).value))" /></span><input :value="intensity" type="range" min="0" max="2" step="0.05" @input="emit('update:intensity', Number(($event.target as HTMLInputElement).value))" /></label>
      <label><span>Movimento <input class="value-input" :value="motion" type="number" min="0" max="2" step="0.05" @input="emit('update:motion', Number(($event.target as HTMLInputElement).value))" /></span><input :value="motion" type="range" min="0" max="2" step="0.05" @input="emit('update:motion', Number(($event.target as HTMLInputElement).value))" /></label>
      <template v-if="kind === 'mascot'">
        <p class="section-label">Mascote</p>
        <label><span>Força de aproximação <input class="value-input" :value="bassSensitivity" type="number" min="0" max="3" step="0.05" @input="emit('update:bassSensitivity', Number(($event.target as HTMLInputElement).value))" /></span><input :value="bassSensitivity" type="range" min="0" max="3" step="0.05" @input="emit('update:bassSensitivity', Number(($event.target as HTMLInputElement).value))" /></label>
        <label><span>Distância do movimento <input class="value-input" :value="bassDistance" type="number" min="0" max="3" step="0.05" @input="emit('update:bassDistance', Number(($event.target as HTMLInputElement).value))" /></span><input :value="bassDistance" type="range" min="0" max="3" step="0.05" @input="emit('update:bassDistance', Number(($event.target as HTMLInputElement).value))" /></label>
        <FrequencyRangeControl label="Bass vermelho" :min-value="bassMinHz" :max-value="bassMaxHz" @update:min-value="emit('update:bassMinHz', $event)" @update:max-value="emit('update:bassMaxHz', $event)" />
        <FrequencyRangeControl label="Energia verde" :min-value="energyMinHz" :max-value="energyMaxHz" @update:min-value="emit('update:energyMinHz', $event)" @update:max-value="emit('update:energyMaxHz', $event)" />
        <FrequencyRangeControl label="Voz roxa" :min-value="voiceMinHz" :max-value="voiceMaxHz" @update:min-value="emit('update:voiceMinHz', $event)" @update:max-value="emit('update:voiceMaxHz', $event)" />
        <FrequencyRangeControl label="Movimento do Mascote" :min-value="motionMinHz" :max-value="motionMaxHz" @update:min-value="emit('update:motionMinHz', $event)" @update:max-value="emit('update:motionMaxHz', $event)" />
      </template>
    </div></div>
  </details>
</template>

<style scoped>
.visualizer-general-controls { position: absolute; top: 14px; right: 14px; z-index: 4; width: 148px; height: 38px; max-height: calc(100% - 28px); padding: 10px 12px; overflow: hidden; border: 1px solid rgba(255, 255, 255, .14); border-radius: 10px; color: rgba(255, 255, 255, .82); background: rgba(5, 8, 12, .78); backdrop-filter: blur(10px); font-size: 11px; transition: width .24s ease, height .24s ease, background .24s ease; scrollbar-width: thin; scrollbar-color: rgba(185, 243, 107, .65) transparent; }
.visualizer-general-controls[open] { width: 330px; height: calc(100% - 28px); overflow-y: auto; }
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
</style>

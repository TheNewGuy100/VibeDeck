<script setup lang="ts">
import { ref } from 'vue';
const props = withDefaults(defineProps<{
  label: string;
  minValue: number;
  maxValue: number;
  minLimit?: number;
  maxLimit?: number;
}>(), { minLimit: 20, maxLimit: 20000 });

const emit = defineEmits<{
  'update:minValue': [value: number];
  'update:maxValue': [value: number];
}>();
const activeHandle = ref<'min' | 'max'>('max');

function setMin(value: number) {
  emit('update:minValue', Math.max(props.minLimit, Math.min(value, props.maxValue - 1)));
}

function setMax(value: number) {
  emit('update:maxValue', Math.min(props.maxLimit, Math.max(value, props.minValue + 1)));
}
</script>

<template>
  <div class="frequency-range-control">
    <span class="frequency-range-label">{{ label }}</span>
    <div class="frequency-values">
      <input :value="minValue" type="number" :min="minLimit" :max="maxValue - 1" step="1" aria-label="Frequência mínima" @input="setMin(Number(($event.target as HTMLInputElement).value))" />
      <span>Hz</span>
      <span>–</span>
      <input :value="maxValue" type="number" :min="minValue + 1" :max="maxLimit" step="1" aria-label="Frequência máxima" @input="setMax(Number(($event.target as HTMLInputElement).value))" />
      <span>Hz</span>
    </div>
    <div class="frequency-dual-slider">
      <input :class="['min-slider', { active: activeHandle === 'min' }]" :value="minValue" type="range" :min="minLimit" :max="maxValue - 1" step="1" aria-label="Slider da frequência mínima" @pointerdown="activeHandle = 'min'" @input="setMin(Number(($event.target as HTMLInputElement).value))" />
      <input :class="['max-slider', { active: activeHandle === 'max' }]" :value="maxValue" type="range" :min="minValue + 1" :max="maxLimit" step="1" aria-label="Slider da frequência máxima" @pointerdown="activeHandle = 'max'" @input="setMax(Number(($event.target as HTMLInputElement).value))" />
    </div>
  </div>
</template>

<style scoped>
.frequency-range-control { display: grid; gap: 5px; margin-top: 10px; }
.frequency-range-label { color: #d4d9e0; font-size: 10px; font-weight: 700; }
.frequency-values { display: flex; align-items: center; gap: 3px; color: #7c8493; font-size: 9px; }
.frequency-values input { width: 48px; padding: 1px 3px; border: 0; border-bottom: 1px solid rgba(185, 243, 107, .65); color: #b9f36b; background: transparent; font: inherit; font-weight: 700; }
.frequency-dual-slider { position: relative; height: 14px; }
.frequency-dual-slider input { width: 100%; position: absolute; inset: 0; margin: 0; pointer-events: none; appearance: none; background: transparent; }
.frequency-dual-slider input::-webkit-slider-runnable-track { height: 3px; border-radius: 3px; background: #353b46; }
.frequency-dual-slider input::-webkit-slider-thumb { width: 12px; height: 12px; margin-top: -4px; border: 0; border-radius: 50%; background: #b9f36b; pointer-events: auto; appearance: none; cursor: pointer; }
.frequency-dual-slider input::-moz-range-track { height: 3px; border-radius: 3px; background: #353b46; }
.frequency-dual-slider input::-moz-range-thumb { width: 12px; height: 12px; border: 0; border-radius: 50%; background: #b9f36b; pointer-events: auto; cursor: pointer; }
.frequency-dual-slider input { z-index: 1; }
.frequency-dual-slider input.active { z-index: 2; }
</style>

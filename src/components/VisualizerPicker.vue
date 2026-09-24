<script setup lang="ts">
import VisualizerCanvas, { type VisualizerKind } from './VisualizerCanvas.vue';

defineProps<{ modelValue: VisualizerKind }>();
const emit = defineEmits<{ 'update:modelValue': [value: VisualizerKind] }>();

</script>

<template>
  <Teleport to=".visualizer-options" defer>
    <button class="visualizer-option-card" :class="{ selected: modelValue === 'spectrogram' }" @click="emit('update:modelValue', 'spectrogram')">
      <span class="visualizer-preview"><VisualizerCanvas kind="spectrogram" compact /></span>
      <span><strong>Espectrograma</strong><small>Frequência ao longo do tempo</small></span>
    </button>
  </Teleport>
</template>

<style scoped>
.visualizer-option-card { min-width: 0; }
.visualizer-option-card :deep(canvas) { width: 100%; height: 100%; }
</style>

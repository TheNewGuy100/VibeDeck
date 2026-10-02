<script setup lang="ts">
import { nextTick, onBeforeUnmount, onErrorCaptured, onMounted, ref } from 'vue';
import VisualizerCanvas, { type VisualizerKind } from './VisualizerCanvas.vue';

defineProps<{ modelValue: VisualizerKind }>();
const emit = defineEmits<{ 'update:modelValue': [value: VisualizerKind] }>();

const teleportTarget = ref<HTMLElement>();
const previewReady = ref(false);
const previewFailed = ref(false);
let targetObserver: MutationObserver | undefined;
let previewFrame = 0;

function resolveTeleportTarget() {
  const target = document.querySelector<HTMLElement>('.visualizer-options');
  teleportTarget.value = target?.isConnected ? target : undefined;
  previewReady.value = false;

  if (teleportTarget.value) {
    previewFrame = requestAnimationFrame(() => {
      if (teleportTarget.value?.isConnected) previewReady.value = true;
    });
  }
}

function handleDomMutation() {
  if (!teleportTarget.value?.isConnected) resolveTeleportTarget();
}

onMounted(async () => {
  await nextTick();
  resolveTeleportTarget();
  targetObserver = new MutationObserver(handleDomMutation);
  targetObserver.observe(document.body, { childList: true, subtree: true });
});

onErrorCaptured(() => {
  previewFailed.value = true;
  return false;
});

onBeforeUnmount(() => {
  cancelAnimationFrame(previewFrame);
  targetObserver?.disconnect();
  teleportTarget.value = undefined;
});
</script>

<template>
  <Teleport v-if="teleportTarget && previewReady" :to="teleportTarget">
    <button class="visualizer-option-card" :class="{ selected: modelValue === 'spectrogram' }" type="button" :aria-pressed="modelValue === 'spectrogram'" @click="emit('update:modelValue', 'spectrogram')">
      <span class="visualizer-preview">
        <VisualizerCanvas v-if="!previewFailed" kind="spectrogram" compact />
        <span v-else class="visualizer-preview-fallback">Preview indisponível</span>
      </span>
      <span><strong>Espectrograma</strong><small>Frequência ao longo do tempo</small></span>
    </button>
  </Teleport>
</template>

<style scoped>
.visualizer-option-card { min-width: 0; }
.visualizer-option-card :deep(canvas) { width: 100%; height: 100%; }
.visualizer-preview-fallback { display: grid; width: 100%; height: 100%; place-items: center; color: rgba(233, 245, 223, .58); font-size: 9px; }
</style>

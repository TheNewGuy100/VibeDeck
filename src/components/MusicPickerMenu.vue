<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';

const emit = defineEmits<{ select: [directory: boolean] }>();
const open = ref(false);
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const menuId = 'add-music-picker-menu';
function close(restoreFocus = false) {
  if (!open.value) return;
  open.value = false;
  if (restoreFocus) requestAnimationFrame(() => trigger.value?.focus());
}
function onDocumentClick(event: MouseEvent) {
  if (!root.value?.contains(event.target as Node)) close();
}
function onDocumentKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && open.value) {
    event.preventDefault();
    close(true);
  }
}
watch(open, (value) => {
  if (value) requestAnimationFrame(() => root.value?.querySelector<HTMLElement>('[role="menuitem"]')?.focus());
});
onMounted(() => {
  document.addEventListener('click', onDocumentClick);
  document.addEventListener('keydown', onDocumentKeydown);
});
onBeforeUnmount(() => {
  document.removeEventListener('click', onDocumentClick);
  document.removeEventListener('keydown', onDocumentKeydown);
});
</script>

<template>
  <div ref="root" class="picker-menu-root">
    <button ref="trigger" class="playlist-drop-hint" type="button" aria-label="Adicionar músicas à playlist" aria-controls="add-music-picker-menu" :aria-expanded="open" aria-haspopup="menu" @click="open = !open">
      <span class="drop-icon" aria-hidden="true">＋</span>
      <strong>Adicione músicas a esta playlist</strong>
      <small>Clique para escolher arquivos ou arraste músicas e pastas para esta área</small>
    </button>
    <div v-if="open" :id="menuId" class="picker-popover" role="menu" aria-label="Adicionar músicas">
      <button type="button" role="menuitem" @click="close(); emit('select', false)">Escolher arquivos</button>
      <button type="button" role="menuitem" @click="close(); emit('select', true)">Escolher pasta</button>
    </div>
  </div>
</template>

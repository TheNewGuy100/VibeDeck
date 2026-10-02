<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';

defineProps<{ connected: boolean }>();
const emit = defineEmits<{ connect: []; disconnect: [] }>();
const open = ref(false);
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const menuId = 'google-account-menu';

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
  <div ref="root" class="account-menu-root">
    <button ref="trigger" class="account-card header-account" type="button" aria-controls="google-account-menu" :aria-expanded="open" aria-haspopup="menu" @click="open = !open">
      <span class="avatar" aria-hidden="true">P</span>
      <span><strong>Conta Google</strong><small>{{ connected ? 'Sincronização ativa' : 'Desconectada' }}</small></span>
      <span :class="['online-dot', { disconnected: !connected }]" :aria-label="connected ? 'Conectada' : 'Desconectada'" />
    </button>
    <div v-if="open" :id="menuId" class="account-popover" role="menu" aria-label="Ações da conta Google">
      <button v-if="connected" type="button" role="menuitem" @click="close(); emit('disconnect')">Desconectar</button>
      <button v-else type="button" role="menuitem" @click="close(); emit('connect')">Conectar / Sincronizar</button>
    </div>
  </div>
</template>

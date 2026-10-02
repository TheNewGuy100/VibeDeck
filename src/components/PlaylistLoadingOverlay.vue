<script setup lang="ts">
withDefaults(defineProps<{
  target?: string;
  title?: string;
  description?: string;
}>(), {
  target: '.track-list',
  title: 'Carregando todas as faixas...',
  description: 'Aguarde enquanto a playlist termina de carregar.',
});
</script>

<template>
  <Teleport :to="target" defer>
    <div
      class="playlist-loading-overlay"
      role="status"
      aria-live="polite"
      aria-busy="true"
      :aria-label="title"
    >
      <span class="loading-spinner" aria-hidden="true"></span>
      <strong>{{ title }}</strong>
      <small>{{ description }}</small>
    </div>
  </Teleport>
</template>

<style scoped>
.playlist-loading-overlay {
  width: 100%;
  min-height: 190px;
  padding: 34px 20px;
  position: sticky;
  top: 0;
  z-index: 5;
  display: grid !important;
  place-content: center;
  justify-items: center;
  gap: 10px;
  color: #d8dce6;
  background: transparent;
  text-align: center;
}

.playlist-loading-overlay strong,
.playlist-loading-overlay small {
  display: block;
  max-width: 100%;
  overflow-wrap: anywhere;
}

.playlist-loading-overlay strong {
  font: 600 16px 'Space Grotesk', sans-serif;
}

.playlist-loading-overlay small {
  color: #7c8493;
  font-size: 10px;
}

.loading-spinner {
  width: 30px;
  height: 30px;
  border: 3px solid #303744;
  border-top-color: #b9f36b;
  border-radius: 50%;
  animation: playlist-loading-spin .8s linear infinite;
}

@keyframes playlist-loading-spin {
  to { transform: rotate(360deg); }
}

@media (prefers-reduced-motion: reduce) {
  .loading-spinner { animation: none; }
}
</style>

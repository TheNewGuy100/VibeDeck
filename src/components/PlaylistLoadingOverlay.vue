<template>
  <Teleport to=".track-list" defer>
    <div class="playlist-loading-overlay" role="status" aria-live="polite">
      <span class="loading-spinner" aria-hidden="true"></span>
      <strong>Carregando todas as faixas...</strong>
      <small>Aguarde enquanto a playlist termina de carregar.</small>
    </div>
  </Teleport>
</template>

<style scoped>
:global(.track-list) { position: relative; }
:global(.playlist-loading + .library-section .track-list) { display: grid !important; }
:global(body.playlist-is-loading) .playlist-loading-overlay { display: grid !important; }
.playlist-loading-overlay { position: absolute; inset: 0; z-index: 5; display: grid; place-content: center; justify-items: center; gap: 10px; color: #d8dce6; background: #0b0e14f2; text-align: center; }
.playlist-loading-overlay :deep(.loading-spinner) { width: 30px; height: 30px; border: 3px solid #303744; border-top-color: #b9f36b; border-radius: 50%; animation: playlist-loading-spin .8s linear infinite; }
.playlist-loading-overlay strong { font: 600 16px 'Space Grotesk'; }
.playlist-loading-overlay small { color: #7c8493; font-size: 10px; }
@keyframes playlist-loading-spin { to { transform: rotate(360deg); } }
</style>

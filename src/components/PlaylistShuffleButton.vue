<script setup lang="ts">
import { Shuffle } from '@lucide/vue';

function shufflePlaylist() {
  window.dispatchEvent(new CustomEvent('vibedeck:shuffle-playlist'));
}
</script>

<template>
  <Teleport to=".section-heading" defer>
    <button class="playlist-shuffle-button" aria-label="Embaralhar playlist" title="Embaralhar playlist" @click="shufflePlaylist">
      <Shuffle :size="15" :stroke-width="1.8" />
    </button>
  </Teleport>
</template>

<style scoped>
.playlist-shuffle-button { width: 28px; height: 28px; margin-left: 6px; border: 1px solid #303744; border-radius: 7px; display: grid; place-items: center; color: #b9f36b; background: #151b18; }
.playlist-shuffle-button:hover { border-color: #b9f36b; background: #1d2c1c; }
</style>

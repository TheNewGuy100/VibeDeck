<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';

export interface QueueTrack {
  id: string;
  title: string;
  artist: string;
  cover: string;
  accent: string;
  source: string;
}

const props = defineProps<{
  open: boolean;
  tracks: QueueTrack[];
  currentTrackId: string | null;
  currentTrackIndex: number;
  isPlaying: boolean;
}>();

const emit = defineEmits<{
  (event: 'update:open', value: boolean): void;
  (event: 'select-track', payload: { queueIndex: number; trackId: string }): void;
}>();

const closeButton = ref<HTMLButtonElement | null>(null);
const triggerButton = ref<HTMLButtonElement | null>(null);
const countLabel = computed(() => `${props.tracks.length} ${props.tracks.length === 1 ? 'faixa' : 'faixas'}`);
function isCurrent(track: QueueTrack, index: number) {
  return props.currentTrackId ? track.id === props.currentTrackId : index === props.currentTrackIndex;
}

function closePanel() {
  emit('update:open', false);
  nextTick(() => triggerButton.value?.focus());
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.stopPropagation();
    closePanel();
  }
}

watch(() => props.open, (isOpen) => {
  if (isOpen) nextTick(() => closeButton.value?.focus());
});
</script>

<template>
  <button
    v-if="open"
    class="queue-backdrop"
    type="button"
    aria-label="Fechar fila de reprodução"
    @click="closePanel"
  />
  <div class="queue-widget" @keydown="handleKeydown">
    <button
      ref="triggerButton"
      class="queue-trigger"
      type="button"
      :aria-label="open ? 'Fechar fila de reprodução' : `Abrir fila de reprodução, ${countLabel}`"
      :aria-expanded="open"
      aria-controls="player-queue-panel"
      @click="emit('update:open', !open)"
    >
      <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round">
        <path d="M4 6h11M4 11h11M4 16h7M18 14v6m-3-3h6" />
      </svg>
      <span class="queue-trigger-count" aria-hidden="true">{{ tracks.length }}</span>
    </button>

    <section
      v-if="open"
      id="player-queue-panel"
      class="queue-panel"
      role="dialog"
      aria-modal="false"
      aria-labelledby="player-queue-title"
    >
      <header class="queue-header">
        <div>
          <span class="queue-eyebrow">TOCANDO A SEGUIR</span>
          <h2 id="player-queue-title">Fila <span>{{ countLabel }}</span></h2>
        </div>
        <button ref="closeButton" class="queue-close" type="button" aria-label="Fechar fila" @click="closePanel">×</button>
      </header>

      <div v-if="tracks.length" class="queue-list" role="list" aria-label="Faixas na fila">
        <button
          v-for="(track, index) in tracks"
          :key="track.id"
          class="queue-row track-row"
          :class="{
            'current-track-match': isCurrent(track, index),
            'current-track-playing': isCurrent(track, index) && isPlaying,
          }"
          type="button"
          role="listitem"
          :aria-current="isCurrent(track, index) ? 'true' : undefined"
          :aria-label="`${index + 1}. ${track.title} — ${track.artist}${isCurrent(track, index) ? (isPlaying ? ', tocando' : ', pausada') : ''}`"
      @click="emit('select-track', { queueIndex: index, trackId: track.id })"
        >
          <span class="track-index">{{ index + 1 }}</span>
          <span class="queue-cover" :style="{ backgroundColor: track.accent }">
            <img v-if="track.cover" :src="track.cover" alt="" />
            <span v-else aria-hidden="true">{{ track.title.slice(0, 1).toUpperCase() }}</span>
          </span>
          <span class="queue-track-info">
            <strong>{{ track.title }}</strong>
            <small>{{ track.artist }}</small>
          </span>
          <span class="queue-source">{{ track.source }}</span>
        </button>
      </div>
      <div v-else class="queue-empty" role="status">
        <span class="queue-empty-icon" aria-hidden="true">♫</span>
        <strong>Sua fila está vazia</strong>
        <span>Adicione uma faixa para continuar ouvindo.</span>
      </div>
    </section>
  </div>
</template>

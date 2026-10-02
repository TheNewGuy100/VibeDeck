<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, watch } from 'vue';
import type { LibraryItem } from '../models/library';

const props = defineProps<{ track: LibraryItem | null }>();
let heading: HTMLElement | null = null;
let mounted = false;

function syncHeadingClass() {
  heading?.classList.toggle('has-track-art', Boolean(props.track?.coverUrl));
}

async function findHeading() {
  await nextTick();
  if (!mounted) return;

  const nextHeading = document.querySelector<HTMLElement>('.visualizer-drawer-heading');
  if (heading !== nextHeading) {
    heading?.classList.remove('has-track-art');
    heading = nextHeading;
  }
  syncHeadingClass();
}

watch(() => props.track?.coverUrl, () => { void findHeading(); });
onMounted(() => {
  mounted = true;
  void findHeading();
});
onBeforeUnmount(() => {
  mounted = false;
  heading?.classList.remove('has-track-art');
  heading = null;
});
</script>

<template>
  <Teleport to=".visualizer-drawer-heading" defer>
    <Transition name="visualizer-cover" mode="out-in">
      <img v-if="track?.coverUrl" :key="track.id" class="visualizer-track-artwork" :src="track.coverUrl" :alt="`Capa de ${track.title}`" draggable="false" />
    </Transition>
  </Teleport>
</template>

<style scoped>
:global(.visualizer-drawer-heading) { position: relative; }
:global(.visualizer-drawer-heading.has-track-art) { min-height: 0; }
:global(.visualizer-drawer-heading > div:first-child) { transition: transform .38s cubic-bezier(.22, .8, .25, 1); }
:global(.visualizer-drawer-heading.has-track-art > div:first-child) { transform: translateX(94px); }
.visualizer-track-artwork { width: 78px; height: 78px; position: absolute; top: 50%; left: 0; border-radius: 9px; object-fit: cover; box-shadow: 0 8px 24px rgba(0, 0, 0, .28); transform: translateY(-50%); }
.visualizer-cover-enter-active, .visualizer-cover-leave-active { transition: transform .38s cubic-bezier(.22, .8, .25, 1), opacity .25s ease; }
.visualizer-cover-enter-from { opacity: 0; transform: translate(100%, -50%); }
.visualizer-cover-leave-to { opacity: 0; transform: translate(-100%, -50%); }
</style>

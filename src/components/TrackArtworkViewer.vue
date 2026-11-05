<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from 'vue';
import type { LibraryItem } from '../models/library';
import { artworkDirection, artworkPointerFromClient, artworkTilt } from './track-artwork-viewer';

const props = defineProps<{ track: LibraryItem | null; origin: HTMLElement | null }>();
const emit = defineEmits<{ close: [] }>();
const card = ref<HTMLElement>();
const artwork = ref<HTMLImageElement>();
const closeButton = ref<HTMLButtonElement>();
const direction = ref('center');
let previousOverflow = '';
let animation: Animation | undefined;
let frame = 0;
let currentX = 0;
let currentY = 0;
let targetX = 0;
let targetY = 0;
let reducedMotion = false;
let mediaQuery: MediaQueryList | undefined;

function animateArtwork(opening: boolean) {
  const image = artwork.value;
  const source = props.origin;
  if (!image || !source || !props.track?.coverUrl || reducedMotion) return;
  const sourceRect = source.getBoundingClientRect();
  const targetRect = image.getBoundingClientRect();
  if (!sourceRect.width || !sourceRect.height || !targetRect.width || !targetRect.height) return;
  const dx = sourceRect.left + sourceRect.width / 2 - (targetRect.left + targetRect.width / 2);
  const dy = sourceRect.top + sourceRect.height / 2 - (targetRect.top + targetRect.height / 2);
  const scale = sourceRect.width / targetRect.width;
  const from = opening ? `translate3d(${dx}px, ${dy}px, 0) scale(${scale})` : 'translate3d(0, 0, 0) scale(1)';
  const to = opening ? 'translate3d(0, 0, 0) scale(1)' : `translate3d(${dx}px, ${dy}px, 0) scale(${scale})`;
  animation?.cancel();
  animation = image.animate([{ transform: from }, { transform: to }], { duration: 590, easing: 'cubic-bezier(.18,.82,.22,1)', fill: 'both' });
}

function tickTilt() {
  frame = 0;
  currentX += (targetX - currentX) * 0.16;
  currentY += (targetY - currentY) * 0.16;
  const x = Math.abs(targetX - currentX) < 0.003 ? targetX : currentX;
  const y = Math.abs(targetY - currentY) < 0.003 ? targetY : currentY;
  currentX = x;
  currentY = y;
  const panel = card.value;
  if (panel) {
    const tilt = artworkTilt({ x, y }, reducedMotion);
    panel.style.setProperty('--tilt-x', `${tilt.rotateX}deg`);
    panel.style.setProperty('--tilt-y', `${tilt.rotateY}deg`);
    panel.style.setProperty('--gloss-x', `${50 + x * 35}%`);
    panel.style.setProperty('--gloss-y', `${50 + y * 35}%`);
  }
  if (x !== targetX || y !== targetY) frame = requestAnimationFrame(tickTilt);
}

function startTilt() { if (!frame && !reducedMotion) frame = requestAnimationFrame(tickTilt); }
function handlePointer(event: PointerEvent) {
  if (!card.value) return;
  const point = artworkPointerFromClient(event.clientX, event.clientY, card.value.getBoundingClientRect());
  targetX = point.x;
  targetY = point.y;
  direction.value = artworkDirection(point.x, point.y);
  startTilt();
}
function resetTilt() { targetX = 0; targetY = 0; direction.value = 'center'; startTilt(); }

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') { event.preventDefault(); emit('close'); return; }
  if (event.key !== 'Tab' || !card.value) return;
  const focusable = card.value.querySelectorAll<HTMLElement>('button:not(:disabled), [tabindex="0"]');
  if (!focusable.length) return;
  const first = focusable[0];
  const last = focusable[focusable.length - 1];
  if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
  else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
}

function restoreFocus() { requestAnimationFrame(() => { if (props.origin?.isConnected) props.origin.focus(); }); }

watch(() => props.track, async (track, previous) => {
  if (track) {
    previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';
    mediaQuery = typeof window.matchMedia === 'function' ? window.matchMedia('(prefers-reduced-motion: reduce)') : undefined;
    reducedMotion = mediaQuery?.matches ?? false;
    await nextTick();
    closeButton.value?.focus();
    animateArtwork(true);
    document.addEventListener('keydown', handleKeydown);
  } else if (previous) {
    document.body.style.overflow = previousOverflow;
    document.removeEventListener('keydown', handleKeydown);
    animation?.cancel();
    restoreFocus();
  }
}, { flush: 'post', immediate: true });

onBeforeUnmount(() => {
  document.body.style.overflow = previousOverflow;
  document.removeEventListener('keydown', handleKeydown);
  if (frame) cancelAnimationFrame(frame);
  animation?.cancel();
});
</script>

<template>
  <Teleport to="body">
    <div v-if="track" class="artwork-viewer-backdrop" role="presentation" @click.self="emit('close')">
      <section ref="card" class="artwork-viewer-card" role="dialog" aria-modal="true" :aria-label="`Capa de ${track.title}`" :data-direction="direction" @pointermove="handlePointer" @pointerleave="resetTilt">
        <div class="artwork-viewer-glass" aria-hidden="true"></div>
        <button ref="closeButton" class="artwork-viewer-close" type="button" aria-label="Fechar capa" @click="emit('close')">×</button>
        <div class="artwork-viewer-image-wrap" :style="{ background: track.accent || '#29313b' }">
          <img v-if="track.coverUrl" ref="artwork" class="artwork-viewer-image" :src="track.coverUrl" :alt="`Capa do álbum ${track.album || track.title}`" draggable="false" />
          <span v-else class="artwork-viewer-fallback" aria-hidden="true">{{ track.title.slice(0, 1) || '♪' }}</span>
        </div>
        <div class="artwork-viewer-caption"><p>{{ track.album || 'FAIXA' }}</p><h2>{{ track.title }}</h2><span>{{ track.artist }}</span></div>
      </section>
    </div>
  </Teleport>
</template>

<style>
.artwork-viewer-backdrop { position: fixed; inset: 0; z-index: 30; display: grid; place-items: center; overflow: auto; padding: max(24px, env(safe-area-inset-top)) 20px max(24px, env(safe-area-inset-bottom)); background: rgba(4, 7, 11, .68); backdrop-filter: blur(15px) saturate(.82); -webkit-backdrop-filter: blur(15px) saturate(.82); }
.artwork-viewer-card { --tilt-x: 0deg; --tilt-y: 0deg; --gloss-x: 50%; --gloss-y: 50%; width: min(450px, calc(100vw - 40px)); max-height: calc(100dvh - 48px); padding: clamp(18px, 4vw, 30px); position: relative; border: 1px solid rgba(255,255,255,.23); border-radius: 25px; color: #f4f5f7; background: linear-gradient(145deg, rgba(255,255,255,.17), rgba(255,255,255,.045) 38%, rgba(13,18,25,.72)), rgba(22,27,36,.78); box-shadow: 0 35px 100px #0009, inset 0 1px rgba(255,255,255,.26); backdrop-filter: blur(28px) saturate(1.35); transform: perspective(1100px) rotateX(var(--tilt-x)) rotateY(var(--tilt-y)); transform-style: preserve-3d; transition: transform .18s ease-out; isolation: isolate; }
.artwork-viewer-glass { position: absolute; inset: 0; z-index: 2; border-radius: inherit; pointer-events: none; background: radial-gradient(ellipse at var(--gloss-x) var(--gloss-y), rgba(255,255,255,.23), transparent 37%), linear-gradient(135deg, rgba(255,255,255,.12), transparent 30%, transparent 70%, rgba(178,232,255,.08)); mix-blend-mode: screen; }
.artwork-viewer-close { width: 38px; height: 38px; display: flex; justify-content: center; align-items: center; position: absolute; z-index: 4; top: 13px; right: 13px; border: 1px solid #ffffff38; border-radius: 50%; color: #fff; background: #111923a8; font-size: 25px; line-height: 1rem; backdrop-filter: blur(12px); }
.artwork-viewer-close:hover { background: #39434e; }.artwork-viewer-close:focus-visible { outline: 2px solid #b9f36b; outline-offset: 3px; }
.artwork-viewer-image-wrap { width: min(100%, 380px); aspect-ratio: 1; margin: 0 auto; position: relative; overflow: hidden; border: 1px solid #ffffff3d; border-radius: 17px; box-shadow: 0 18px 48px #0008, inset 0 1px #ffffff40; }
.artwork-viewer-image { width: 100%; height: 100%; position: absolute; inset: 0; display: block; object-fit: cover; will-change: transform; box-shadow: 0 18px 55px #0008; }
.artwork-viewer-fallback { width: 100%; height: 100%; display: grid; place-items: center; color: #ffffffd9; font: 600 clamp(100px, 28vw, 190px)/1 'Space Grotesk', sans-serif; text-shadow: 0 10px 38px #0006; }
.artwork-viewer-caption { padding: 20px 4px 0; }.artwork-viewer-caption p { margin: 0 0 6px; color: #b9f36b; font-size: 9px; font-weight: 700; letter-spacing: 1.5px; text-transform: uppercase; }.artwork-viewer-caption h2 { max-width: 100%; margin: 0; overflow: hidden; font: 600 clamp(20px, 5vw, 27px)/1.2 'Space Grotesk', sans-serif; text-overflow: ellipsis; white-space: nowrap; }.artwork-viewer-caption span { display: block; margin-top: 7px; color: #b8c0cb; font-size: 12px; }
.light-theme .artwork-viewer-card { color: #20252e; border-color: #ffffffb0; background: linear-gradient(145deg,#ffffffc9,#f8faf2d9 52%,#dce8e6c9); box-shadow: 0 35px 100px #34402d55,inset 0 1px #fff; }.light-theme .artwork-viewer-caption span { color: #5d6875; }.light-theme .artwork-viewer-close { color: #283126; background: #ffffffa6; }
@media (max-width: 600px) { .artwork-viewer-backdrop { padding-right: 14px; padding-left: 14px; }.artwork-viewer-card { width: min(420px, calc(100vw - 28px)); padding: 16px; border-radius: 21px; }.artwork-viewer-image-wrap { width: 100%; border-radius: 13px; }.artwork-viewer-close { top: 9px; right: 9px; } }
@media (prefers-reduced-motion: reduce) { .artwork-viewer-card { transition: none; transform: none; }.artwork-viewer-image { will-change: auto; } }
</style>

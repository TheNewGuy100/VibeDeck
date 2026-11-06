import { mount } from '@vue/test-utils';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import TrackArtworkViewer from './TrackArtworkViewer.vue';
import { artworkDirection, artworkPointerFromClient, artworkTilt, normalizeArtworkPointer } from './track-artwork-viewer';

const track = { id: 'a1', title: 'Glass Garden', artist: 'Vibe Artist', album: 'Afterlight', duration: '3:10', source: 'local' as const, accent: '#6c4a91', coverUrl: 'http://localhost:1420/covers/afterlight.jpg' };
afterEach(() => { document.body.innerHTML = ''; document.body.style.overflow = ''; vi.restoreAllMocks(); });

describe('artwork pointer helpers', () => {
  it('maps all eight pointer octants and the neutral center', () => {
    expect(artworkDirection(0, 0)).toBe('center');
    expect([[0, -1], [1, -1], [1, 0], [1, 1], [0, 1], [-1, 1], [-1, 0], [-1, -1]].map(([x, y]) => artworkDirection(x!, y!))).toEqual(['up', 'up-right', 'right', 'down-right', 'down', 'down-left', 'left', 'up-left']);
  });
  it('normalizes and clamps coordinates, maps client position, and neutralizes degenerate bounds', () => {
    expect(normalizeArtworkPointer(4, -3)).toEqual({ x: 1, y: -1 });
    expect(artworkPointerFromClient(150, 75, { left: 50, top: 25, width: 100, height: 50 })).toEqual({ x: 1, y: 1 });
    expect(artworkPointerFromClient(0, 0, { left: 0, top: 0, width: 0, height: 0 })).toEqual({ x: 0, y: 0 });
  });
  it('disables tilt under reduced motion', () => {
    expect(artworkTilt({ x: .7, y: -.5 }, true)).toEqual({ rotateX: 0, rotateY: 0 });
    expect(artworkTilt({ x: .5, y: -1 }, false)).toEqual({ rotateX: 8, rotateY: 4 });
  });
});

describe('TrackArtworkViewer', () => {
  it('keeps row playback separate from the cover button and defines hover/focus/reduced-motion states', () => {
    const app = readFileSync(resolve(process.cwd(), 'src/App.vue'), 'utf8');
    const styles = readFileSync(resolve(process.cwd(), 'src/styles.css'), 'utf8');
    const viewer = readFileSync(resolve(process.cwd(), 'src/components/TrackArtworkViewer.vue'), 'utf8');
    expect(app).toMatch(/:class="\['track-row', \{ 'track-unavailable': isTrackUnavailable\(item\) \}\]"[^>]*role="button" tabindex="0" @click="play\(/);
    expect(app).toMatch(/class="cover cover-button" type="button" :aria-label="`Abrir capa de \$\{item\.title\}`" @click\.stop="openArtwork\(item, \$event\)" @keydown\.stop/);
    expect(app).toMatch(/class="favorite-button"[^>]*@click\.stop="toggleFavorite\(item\)"/);
    expect(viewer).toMatch(/\.artwork-viewer-close\s*\{[^}]*display:\s*flex;[^}]*justify-content:\s*center;[^}]*align-items:\s*center;[^}]*line-height:\s*1rem;/);
    expect(styles).toContain('.cover-button:hover, .cover-button:focus-visible');
    expect(styles).toContain('@media (prefers-reduced-motion: reduce) { .cover-button');
  });

  it('opens an accessible dialog without playback behavior, closes by backdrop/Escape and restores origin focus', async () => {
    const origin = document.createElement('button');
    origin.setAttribute('aria-label', 'Abrir capa de Glass Garden');
    document.body.append(origin);
    const wrapper = mount(TrackArtworkViewer, { attachTo: document.body, props: { track, origin } });
    await wrapper.vm.$nextTick();
    await wrapper.vm.$nextTick();
    expect(document.querySelector('[role="dialog"]')?.getAttribute('aria-label')).toBe('Capa de Glass Garden');
    expect(document.activeElement).toBe(document.querySelector('.artwork-viewer-close'));
    expect(document.body.style.overflow).toBe('hidden');
    document.querySelector('.artwork-viewer-backdrop')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(wrapper.emitted('close')).toHaveLength(1);
    await wrapper.setProps({ track: null });
    await new Promise((resolve) => requestAnimationFrame(resolve));
    expect(document.activeElement).toBe(origin);
    expect(document.body.style.overflow).toBe('');
    await wrapper.setProps({ track, origin });
    await wrapper.vm.$nextTick();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(wrapper.emitted('close')).toHaveLength(2);
    wrapper.unmount();
  });

  it('renders accent/title fallback for a track without cover art and pointer motion updates direction', async () => {
    const origin = document.createElement('button');
    document.body.append(origin);
    const wrapper = mount(TrackArtworkViewer, { attachTo: document.body, props: { track: { ...track, coverUrl: undefined }, origin } });
    await wrapper.vm.$nextTick();
    expect(document.querySelector('.artwork-viewer-fallback')?.textContent).toBe('G');
    expect(document.querySelector('.artwork-viewer-image')).toBeNull();
    const card = document.querySelector<HTMLElement>('[role="dialog"]')!;
    vi.spyOn(card, 'getBoundingClientRect').mockReturnValue({ left: 0, top: 0, width: 200, height: 200 } as DOMRect);
    card.dispatchEvent(new MouseEvent('pointermove', { clientX: 200, clientY: 100, bubbles: true }));
    await wrapper.vm.$nextTick();
    expect(card.getAttribute('data-direction')).toBe('right');
    card.dispatchEvent(new MouseEvent('pointerleave'));
    await wrapper.vm.$nextTick();
    expect(card.getAttribute('data-direction')).toBe('center');
    wrapper.unmount();
  });
});

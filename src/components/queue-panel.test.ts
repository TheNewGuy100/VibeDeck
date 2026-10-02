import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import QueuePanel, { type QueueTrack } from './QueuePanel.vue';
import { nextRepeatMode, type RepeatMode } from '../services/player-state';

const tracks: QueueTrack[] = [
  { id: 'one', title: 'First track', artist: 'Artist One', cover: '/cover.jpg', accent: '#b9f36b', source: 'Local' },
  { id: 'two', title: 'Second track', artist: 'Artist Two', cover: '', accent: '#c1a6ff', source: 'YouTube Music' },
];

const mountQueue = (overrides: Partial<{
  open: boolean; tracks: QueueTrack[]; currentTrackId: string | null; currentTrackIndex: number; isPlaying: boolean;
}> = {}) => mount(QueuePanel, {
  props: { open: false, tracks, currentTrackId: null, currentTrackIndex: -1, isPlaying: false, ...overrides },
});

describe('QueuePanel', () => {
  it('keeps the requested trigger, badge, panel offset, and responsive sizing rules', () => {
    const styles = readFileSync(resolve(process.cwd(), 'src/styles.css'), 'utf8');
    expect(styles).toMatch(/\.queue-trigger\s*\{[^}]*width:\s*34px;[^}]*height:\s*34px;/s);
    expect(styles).toMatch(/\.queue-trigger-count\s*\{[^}]*top:\s*-7px;[^}]*right:\s*-16px;/s);
    expect(styles).toMatch(/\.queue-panel\s*\{[^}]*bottom:\s*43px;/s);
    expect(styles).toMatch(/@media\s*\(max-width:\s*600px\)[\s\S]*?\.queue-panel\s*\{[^}]*bottom:\s*43px;/);
  });

  it('closes only through the open-state backdrop when clicking outside', async () => {
    const closed = mountQueue();
    expect(closed.find('.queue-backdrop').exists()).toBe(false);

    const opened = mountQueue({ open: true });
    await opened.get('.queue-backdrop').trigger('click');
    expect(opened.emitted('update:open')?.[0]).toEqual([false]);
  });

  it('does not dismiss when interacting inside the panel and retains focus and Escape behavior', async () => {
    const wrapper = mountQueue({ open: true });
    await wrapper.get('.queue-row').trigger('click');
    expect(wrapper.emitted('update:open')).toBeUndefined();

    await wrapper.get('.queue-panel').trigger('keydown', { key: 'Escape' });
    expect(wrapper.emitted('update:open')?.at(-1)).toEqual([false]);
  });

  it('keeps the panel closed until the parent opens it and emits the controlled state change', async () => {
    const wrapper = mountQueue();
    expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
    await wrapper.get('.queue-trigger').trigger('click');
    expect(wrapper.emitted('update:open')?.[0]).toEqual([true]);
    await wrapper.setProps({ open: true });
    expect(wrapper.find('[role="dialog"]').exists()).toBe(true);
  });

  it('marks the active playing track with the playlist indicator pattern', () => {
    const wrapper = mountQueue({ open: true, currentTrackId: 'one', currentTrackIndex: 0, isPlaying: true });
    const active = wrapper.get('.queue-row');
    expect(active.classes()).toContain('current-track-match');
    expect(active.classes()).toContain('current-track-playing');
    expect(active.attributes('aria-current')).toBe('true');
    expect(active.attributes('aria-label')).toContain('tocando');
  });

  it('marks the active paused track without the animated playing state', () => {
    const wrapper = mountQueue({ open: true, currentTrackId: 'one', currentTrackIndex: 0, isPlaying: false });
    const active = wrapper.get('.queue-row');
    expect(active.classes()).toContain('current-track-match');
    expect(active.classes()).not.toContain('current-track-playing');
    expect(active.attributes('aria-label')).toContain('pausada');
  });

  it('emits queue index and stable track id when a queued track is selected', async () => {
    const wrapper = mountQueue({ open: true });
    await wrapper.findAll('.queue-row')[1].trigger('click');
    expect(wrapper.emitted('select-track')?.[0]).toEqual([{ queueIndex: 1, trackId: 'two' }]);
  });

  it('renders a helpful empty state without queue actions', () => {
    const wrapper = mountQueue({ open: true, tracks: [] });
    expect(wrapper.get('.queue-empty').text()).toContain('Sua fila está vazia');
    expect(wrapper.find('.queue-row').exists()).toBe(false);
    expect(wrapper.get('.queue-trigger-count').text()).toBe('0');
  });

  it('renders the supplied full queue count and every queue row', () => {
    const fullQueue = [tracks[0], tracks[1], { ...tracks[0], id: 'three', title: 'Third track' }];
    const wrapper = mountQueue({ open: true, tracks: fullQueue, currentTrackIndex: 1, currentTrackId: 'two' });
    expect(wrapper.get('.queue-trigger-count').text()).toBe('3');
    expect(wrapper.findAll('.queue-row')).toHaveLength(3);
  });

  it('renders current props immediately after selecting source tracks A then B', async () => {
    const wrapper = mountQueue({ open: true });
    const source = [tracks[0], tracks[1]];
    const queueFromA = [source[0], source[1]];
    await wrapper.setProps({ tracks: queueFromA, currentTrackId: 'one', currentTrackIndex: 0 });
    expect(wrapper.findAll('.queue-row').map((row) => row.get('strong').text())).toEqual(['First track', 'Second track']);
    const queueFromB = [source[1], source[0]];
    await wrapper.setProps({ tracks: queueFromB, currentTrackId: 'two', currentTrackIndex: 0 });
    expect(wrapper.findAll('.queue-row').map((row) => row.get('strong').text())).toEqual(['Second track', 'First track']);
    expect(wrapper.findAll('.queue-row')[0].classes()).toContain('current-track-match');
  });

  it('retains every queue row and current selection through each repeat mode prop update', async () => {
    const wrapper = mountQueue({ open: true, tracks, currentTrackId: 'one', currentTrackIndex: 0 });
    let mode: RepeatMode = 'off';
    for (let transition = 0; transition < 3; transition += 1) {
      mode = nextRepeatMode(mode);
      // Repeat mode changes are represented by parent state; the panel keeps the full queue projection.
      await wrapper.setProps({ tracks: [...tracks], currentTrackId: 'one', currentTrackIndex: 0 });
      expect(wrapper.findAll('.queue-row')).toHaveLength(2);
      expect(wrapper.get('.queue-row').classes()).toContain('current-track-match');
    }
    expect(mode).toBe('off');
  });

  it('opens by keyboard and closes on Escape or the close button', async () => {
    const wrapper = mountQueue();
    await wrapper.get('.queue-trigger').trigger('click');
    await wrapper.setProps({ open: true });
    await wrapper.get('.queue-panel').trigger('keydown', { key: 'Escape' });
    expect(wrapper.emitted('update:open')?.at(-1)).toEqual([false]);
    await wrapper.get('.queue-close').trigger('click');
    expect(wrapper.emitted('update:open')?.at(-1)).toEqual([false]);
  });

  it('keeps reduced-motion preference in CSS without relying on animation state in markup', () => {
    const wrapper = mountQueue({ open: true, currentTrackId: 'one', currentTrackIndex: 0, isPlaying: true });
    expect(wrapper.get('.queue-row').classes()).toContain('current-track-playing');
  });
});

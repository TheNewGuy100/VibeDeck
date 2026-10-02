import { nextTick } from 'vue';
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import VisualizerTrackArtwork from './VisualizerTrackArtwork.vue';

const track = { id: '1', title: 'Song', artist: 'Artist', album: 'Album', duration: '1:00', source: 'youtube' as const, accent: '#000', coverUrl: 'cover-a.jpg' };

describe('VisualizerTrackArtwork', () => {
  it('keeps the heading position without artwork and shifts it when artwork exists', async () => {
    document.body.innerHTML = '<div class="visualizer-drawer-heading"><div><h2>Song</h2></div></div>';
    const wrapper = mount(VisualizerTrackArtwork, { props: { track: null } });
    await nextTick();
    expect(document.querySelector('.visualizer-drawer-heading')?.classList.contains('has-track-art')).toBe(false);

    await wrapper.setProps({ track });
    await nextTick();
    expect(document.querySelector('.visualizer-drawer-heading')?.classList.contains('has-track-art')).toBe(true);
    expect(document.querySelector('.visualizer-track-artwork')?.getAttribute('src')).toBe('cover-a.jpg');
    wrapper.unmount();
  });
});

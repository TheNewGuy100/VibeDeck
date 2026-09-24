import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import VisualizerControls from './VisualizerControls.vue';

const baseProps = {
  sensitivity: 1,
  smoothing: 0.5,
  intensity: 1,
  motion: 1,
  kind: 'mascot' as const,
  bassSensitivity: 1,
  bassDistance: 1,
  bassMinHz: 20,
  bassMaxHz: 250,
  energyMinHz: 20,
  energyMaxHz: 16000,
  voiceMinHz: 180,
  voiceMaxHz: 3500,
  motionMinHz: 20,
  motionMaxHz: 250,
  visible: true,
};

describe('VisualizerControls', () => {
  it('starts compact and expands to show the controls', async () => {
    const wrapper = mount(VisualizerControls, { props: baseProps });
    const panel = wrapper.find('details');
    expect(panel.attributes('open')).toBeDefined();
    expect(wrapper.find('.spectrogram-button').text()).toContain('Abrir espectrograma');
    await panel.trigger('click');
    expect(wrapper.find('.controls-body').exists()).toBe(true);
  });

  it('toggles the spectrogram action label', async () => {
    const wrapper = mount(VisualizerControls, { props: baseProps });
    const button = wrapper.find('.spectrogram-button');
    await button.trigger('click');
    expect(button.text()).toContain('Voltar para visualizer');
    await button.trigger('click');
    expect(button.text()).toContain('Abrir espectrograma');
  });
});

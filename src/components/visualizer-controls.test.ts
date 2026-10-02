import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import VisualizerControls from '../visualizers/VisualizerControls.vue';
import { defaultSerpentLights, defaultSnakeLayout } from '../visualizers/serpents-layout';
import { serpentsLights, serpentsSnakes } from '../services/serpents-preferences';

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

  it('shows Serpents groups only for that visualizer and keeps other controls intact', async () => {
    const wrapper = mount(VisualizerControls, { props: { ...baseProps, kind: 'serpents' } });
    expect(wrapper.text()).toContain('Serpentes · Cobras');
    expect(wrapper.text()).toContain('Serpentes · Iluminação');
    await wrapper.setProps({ kind: 'bars' });
    expect(wrapper.text()).not.toContain('Serpentes · Cobras');
    expect(wrapper.text()).toContain('Sensibilidade');
  });

  it('adds and removes snakes/lights within supported bounds', async () => {
    serpentsSnakes.value = defaultSnakeLayout();
    serpentsLights.value = defaultSerpentLights();
    const wrapper = mount(VisualizerControls, { props: { ...baseProps, kind: 'serpents' } });
    const addSnakeButton = wrapper.findAll('button').find(button => button.text() === 'Adicionar cobra')!;
    for (let count = serpentsSnakes.value.length; count < 8; count++) await addSnakeButton.trigger('click');
    expect(serpentsSnakes.value).toHaveLength(8);
    expect(addSnakeButton.attributes('disabled')).toBeDefined();
    for (let count = 8; count > 2; count--) await wrapper.find(`[aria-label="Remover cobra ${count}"]`).trigger('click');
    expect(serpentsSnakes.value).toHaveLength(2);
    expect(wrapper.find('[aria-label="Remover cobra 1"]').attributes('disabled')).toBeDefined();

    const addLightButton = wrapper.findAll('button').find(button => button.text() === 'Adicionar luz-chave')!;
    for (let count = serpentsLights.value.length; count < 8; count++) await addLightButton.trigger('click');
    expect(serpentsLights.value).toHaveLength(8);
    expect(addLightButton.attributes('disabled')).toBeDefined();
    for (let count = 8; count > 1; count--) await wrapper.find(`[aria-label="Remover luz ${count}"]`).trigger('click');
    expect(serpentsLights.value).toHaveLength(1);
    expect(wrapper.find('[aria-label="Remover luz 1"]').attributes('disabled')).toBeDefined();
  });
});

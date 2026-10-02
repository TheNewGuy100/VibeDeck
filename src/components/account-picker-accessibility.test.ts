import { mount } from '@vue/test-utils';
import { nextTick } from 'vue';
import { afterEach, describe, expect, it, vi } from 'vitest';
import AccountMenu from './AccountMenu.vue';
import MusicPickerMenu from './MusicPickerMenu.vue';

afterEach(() => { document.body.innerHTML = ''; });

describe('AccountMenu', () => {
  it('toggles from the account card without connecting; disconnected state exposes only connect/sync', async () => {
    const connect = vi.fn();
    const disconnect = vi.fn();
    const wrapper = mount(AccountMenu, { props: { connected: false }, attachTo: document.body });
    const trigger = wrapper.get('.account-card');
    expect(trigger.attributes('aria-expanded')).toBe('false');
    expect(trigger.attributes('aria-controls')).toBe('google-account-menu');
    await trigger.trigger('click');
    expect(wrapper.get('.account-card').attributes('aria-expanded')).toBe('true');
    expect(wrapper.findAll('[role="menuitem"]').map((item) => item.text())).toEqual(['Conectar / Sincronizar']);
    expect(connect).not.toHaveBeenCalled();
    expect(disconnect).not.toHaveBeenCalled();
    await wrapper.get('[role="menuitem"]').trigger('click');
    expect(wrapper.emitted('connect')).toHaveLength(1);
    expect(wrapper.emitted('disconnect')).toBeUndefined();
    wrapper.unmount();
  });

  it('shows only disconnect while connected and closes on outside click/Escape with focus restoration', async () => {
    const wrapper = mount(AccountMenu, { props: { connected: true }, attachTo: document.body });
    const trigger = wrapper.get<HTMLButtonElement>('.account-card');
    await trigger.trigger('click');
    expect(wrapper.findAll('[role="menuitem"]').map((item) => item.text())).toEqual(['Desconectar']);
    document.body.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await nextTick();
    expect(wrapper.find('[role="menu"]').exists()).toBe(false);
    await trigger.trigger('click');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await nextTick();
    expect(wrapper.find('[role="menu"]').exists()).toBe(false);
    await new Promise((resolve) => requestAnimationFrame(resolve));
    expect(document.activeElement).toBe(trigger.element);
    wrapper.unmount();
  });
});

describe('MusicPickerMenu', () => {
  it('exposes its trigger label and expanded-controls relationship, emits the correct file/folder choice', async () => {
    const wrapper = mount(MusicPickerMenu, { attachTo: document.body });
    const trigger = wrapper.get('.playlist-drop-hint');
    expect(trigger.attributes('aria-label')).toBe('Adicionar músicas à playlist');
    expect(trigger.attributes('aria-controls')).toBe('add-music-picker-menu');
    expect(trigger.attributes('aria-expanded')).toBe('false');
    await trigger.trigger('click');
    expect(trigger.attributes('aria-expanded')).toBe('true');
    expect(wrapper.findAll('[role="menuitem"]').map((item) => item.text())).toEqual(['Escolher arquivos', 'Escolher pasta']);
    await wrapper.findAll('[role="menuitem"]')[0].trigger('click');
    expect(wrapper.emitted('select')?.[0]).toEqual([false]);
    await trigger.trigger('click');
    await wrapper.findAll('[role="menuitem"]')[1].trigger('click');
    expect(wrapper.emitted('select')?.[1]).toEqual([true]);
    wrapper.unmount();
  });

  it('dismisses outside and by Escape, returning focus to the picker trigger on Escape', async () => {
    const wrapper = mount(MusicPickerMenu, { attachTo: document.body });
    const trigger = wrapper.get<HTMLButtonElement>('.playlist-drop-hint');
    await trigger.trigger('click');
    document.body.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await nextTick();
    expect(wrapper.find('[role="menu"]').exists()).toBe(false);
    await trigger.trigger('click');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await nextTick();
    expect(wrapper.find('[role="menu"]').exists()).toBe(false);
    await new Promise((resolve) => requestAnimationFrame(resolve));
    expect(document.activeElement).toBe(trigger.element);
    expect(wrapper.emitted('select')).toBeUndefined();
    wrapper.unmount();
  });
});

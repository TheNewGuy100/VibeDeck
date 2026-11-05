import { afterEach, describe, expect, it, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import PlayerSandboxFrame from './PlayerSandboxFrame.vue';

const baseProps = {
  connected: true,
  busy: false,
  status: 'Player pronto para receber uma solicitação manual.',
};

function dispatchFrameMessage(wrapper: ReturnType<typeof mount>, data: unknown, source?: MessageEventSource | null) {
  const frame = wrapper.find('iframe').element as HTMLIFrameElement;
  window.dispatchEvent(new MessageEvent('message', {
    data,
    source: source === undefined ? frame.contentWindow : source,
  }));
}

describe('PlayerSandboxFrame', () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  it('uses an opaque scripts-only iframe and never starts playback by itself', () => {
    const wrapper = mount(PlayerSandboxFrame, { props: baseProps, attachTo: document.body });

    expect(wrapper.find('iframe').attributes('sandbox')).toBe('allow-scripts');
    expect(wrapper.emitted('play')).toBeUndefined();
    wrapper.unmount();
  });

  it('emits only an exact 11-character video ID from its own frame', () => {
    const wrapper = mount(PlayerSandboxFrame, { props: baseProps, attachTo: document.body });
    dispatchFrameMessage(wrapper, { type: 'player-sandbox:play', version: 1, videoId: 'Abc_123-xyz' });

    expect(wrapper.emitted('play')).toEqual([['Abc_123-xyz']]);
    wrapper.unmount();
  });

  it('rejects malformed, extra-field, invalid-ID and foreign-frame messages', () => {
    const wrapper = mount(PlayerSandboxFrame, { props: baseProps, attachTo: document.body });
    dispatchFrameMessage(wrapper, { type: 'player-sandbox:play', version: 1, videoId: 'short' });
    dispatchFrameMessage(wrapper, { type: 'player-sandbox:play', version: 1, videoId: 'Abc_123-xyz', url: 'https://example.invalid' });
    dispatchFrameMessage(wrapper, { type: 'player-sandbox:play', version: 1, videoId: 'Abc_123-xyz' }, window);

    expect(wrapper.emitted('play')).toBeUndefined();
    wrapper.unmount();
  });

  it('rejects playback requests when disconnected or already busy', async () => {
    const wrapper = mount(PlayerSandboxFrame, { props: { ...baseProps, connected: false }, attachTo: document.body });
    dispatchFrameMessage(wrapper, { type: 'player-sandbox:play', version: 1, videoId: 'Abc_123-xyz' });
    await wrapper.setProps({ connected: true, busy: true });
    dispatchFrameMessage(wrapper, { type: 'player-sandbox:play', version: 1, videoId: 'Abc_123-xyz' });

    expect(wrapper.emitted('play')).toBeUndefined();
    wrapper.unmount();
  });

  it('relays safe host diagnostics into the frame error status', async () => {
    const wrapper = mount(PlayerSandboxFrame, { props: baseProps, attachTo: document.body });
    const frame = wrapper.find('iframe').element as HTMLIFrameElement;
    const postMessage = vi.spyOn(frame.contentWindow!, 'postMessage').mockImplementation(() => undefined);

    await wrapper.setProps({ status: 'Falha nativa: código=codec_error; etapa=failed; decoder=não; saída=não; mídia=732198 bytes.' });

    expect(postMessage).toHaveBeenLastCalledWith(expect.objectContaining({
      type: 'player-sandbox:status',
      status: 'error',
      summary: 'Falha nativa: código=codec_error; etapa=failed; decoder=não; saída=não; mídia=732198 bytes.',
    }), '*');
    wrapper.unmount();
  });

  it('redacts unsafe diagnostic text before relaying it to the frame', async () => {
    const wrapper = mount(PlayerSandboxFrame, { props: baseProps, attachTo: document.body });
    const frame = wrapper.find('iframe').element as HTMLIFrameElement;
    const postMessage = vi.spyOn(frame.contentWindow!, 'postMessage').mockImplementation(() => undefined);

    await wrapper.setProps({ status: 'Falha nativa: https://private.invalid/signed?token=secret' });

    expect(postMessage).toHaveBeenLastCalledWith(expect.objectContaining({
      type: 'player-sandbox:status',
      status: 'error',
      summary: 'O status do player foi ocultado por segurança.',
    }), '*');
    wrapper.unmount();
  });
});

import { describe, expect, it, vi } from 'vitest';
import { loginErrorCode, loginErrorMessage, waitForLogin, YouTubeLoginError, type LoginEvent } from './youtube-login';

const noSleep = () => Promise.resolve();

describe('loginErrorCode', () => {
  it('extracts the native code from strings and errors', () => {
    expect(loginErrorCode('youtube_cookie_capture_failed')).toBe('youtube_cookie_capture_failed');
    expect(loginErrorCode(new Error('failed: youtube_login_build_failed'))).toBe('youtube_login_build_failed');
    expect(loginErrorCode('A janela de login do YouTube Music não está aberta.')).toBe('youtube_login_window_closed');
    expect(loginErrorCode(undefined)).toBe('youtube_login_unknown');
  });

  it('gives actionable messages and a fallback that names the code', () => {
    expect(loginErrorMessage('youtube_login_window_closed')).toContain('fechada');
    expect(loginErrorMessage('youtube_foo')).toContain('youtube_foo');
  });
});

describe('waitForLogin', () => {
  it('keeps waiting while not signed in and finishes once the session appears', async () => {
    const capture = vi.fn<() => Promise<boolean>>().mockResolvedValueOnce(false).mockResolvedValueOnce(false).mockResolvedValue(true);
    const events: LoginEvent[] = [];
    await waitForLogin(capture, { sleep: noSleep, onEvent: (event) => events.push(event) });
    expect(capture).toHaveBeenCalledTimes(3);
    expect(events.at(-1)).toMatchObject({ kind: 'connected', attempts: 3 });
  });

  it('does not fail on the first "not ready" errors while the login window is still loading', async () => {
    const capture = vi.fn<() => Promise<boolean>>()
      .mockRejectedValueOnce('youtube_cookie_capture_failed')
      .mockRejectedValueOnce('youtube_cookie_dispatch_failed')
      .mockResolvedValueOnce(false)
      .mockResolvedValue(true);
    const events: LoginEvent[] = [];
    await waitForLogin(capture, { sleep: noSleep, onEvent: (event) => events.push(event) });
    expect(events.filter((event) => event.kind === 'transient')).toHaveLength(2);
    expect(events.at(-1)?.kind).toBe('connected');
  });

  it('gives up with the cause after too many consecutive "not ready" errors, but a good answer resets the count', async () => {
    const failing = vi.fn(() => Promise.reject('youtube_cookie_capture_timeout'));
    await expect(waitForLogin(failing, { sleep: noSleep, maxTransient: 3 })).rejects.toMatchObject({ code: 'youtube_cookie_capture_timeout' });
    expect(failing).toHaveBeenCalledTimes(4);

    const flaky = vi.fn<() => Promise<boolean>>()
      .mockRejectedValueOnce('youtube_cookie_capture_failed').mockRejectedValueOnce('youtube_cookie_capture_failed').mockResolvedValueOnce(false)
      .mockRejectedValueOnce('youtube_cookie_capture_failed').mockRejectedValueOnce('youtube_cookie_capture_failed').mockResolvedValue(true);
    await expect(waitForLogin(flaky, { sleep: noSleep, maxTransient: 2 })).resolves.toBeUndefined();
  });

  it('keeps waiting through cookie-content errors so the user can finish signing in', async () => {
    const capture = vi.fn<() => Promise<boolean>>()
      .mockRejectedValueOnce('youtube_session_ambiguous_cookie')
      .mockRejectedValueOnce('youtube_session_ambiguous_cookie')
      .mockResolvedValueOnce(false)
      .mockResolvedValue(true);
    const events: LoginEvent[] = [];
    await waitForLogin(capture, { sleep: noSleep, maxTransient: 1, onEvent: (event) => events.push(event) });
    expect(events.filter((event) => event.kind === 'transient')).toHaveLength(1);
    expect(events.at(-1)?.kind).toBe('connected');
  });

  it('reports a cookie-content error that never clears as the cause at the time limit', async () => {
    await expect(waitForLogin(() => Promise.reject('youtube_session_ambiguous_cookie'), { sleep: noSleep, timeoutMs: 5000 }))
      .rejects.toMatchObject({ code: 'youtube_session_ambiguous_cookie' });
  });

  it('stops at once when the window was closed or the error is not a "not ready" one', async () => {
    const closed = vi.fn(() => Promise.reject('A janela de login do YouTube Music não está aberta.'));
    await expect(waitForLogin(closed, { sleep: noSleep })).rejects.toMatchObject({ code: 'youtube_login_window_closed' });
    expect(closed).toHaveBeenCalledTimes(1);
    const other = vi.fn(() => Promise.reject('youtube_session_write_failed'));
    await expect(waitForLogin(other, { sleep: noSleep })).rejects.toBeInstanceOf(YouTubeLoginError);
    expect(other).toHaveBeenCalledTimes(1);
  });

  it('times out with its own code and reports progress', async () => {
    const events: LoginEvent[] = [];
    await expect(waitForLogin(() => Promise.resolve(false), { sleep: noSleep, intervalMs: 1000, timeoutMs: 65_000, onEvent: (event) => events.push(event) }))
      .rejects.toMatchObject({ code: 'youtube_login_timeout' });
    expect(events.filter((event) => event.kind === 'waiting').map((event) => (event as { elapsedSeconds: number }).elapsedSeconds)).toEqual([30, 60]);
    expect(events.at(-1)).toEqual({ kind: 'failed', code: 'youtube_login_timeout' });
  });

  it('waits before the first check', async () => {
    const delays: number[] = [];
    await waitForLogin(() => Promise.resolve(true), { sleep: (ms) => { delays.push(ms); return Promise.resolve(); }, initialDelayMs: 1500 });
    expect(delays[0]).toBe(1500);
  });
});

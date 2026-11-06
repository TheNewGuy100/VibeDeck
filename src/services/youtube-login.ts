/** Waiting for the user to finish signing in to YouTube Music in the login window. */

export type LoginEvent =
  | { kind: 'waiting'; elapsedSeconds: number }
  | { kind: 'transient'; code: string; streak: number }
  | { kind: 'connected'; attempts: number; elapsedSeconds: number }
  | { kind: 'failed'; code: string };

export class YouTubeLoginError extends Error {
  readonly code: string;
  constructor(code: string, message = loginErrorMessage(code)) {
    super(message);
    this.name = 'YouTubeLoginError';
    this.code = code;
  }
}

/** Errors that only mean "the login window is not ready yet" while it is still loading. */
export const TRANSIENT_LOGIN_CODES: ReadonlySet<string> = new Set([
  'youtube_cookie_capture_failed',
  'youtube_cookie_capture_timeout',
  'youtube_cookie_dispatch_failed',
]);

/**
 * Cookie-content problems that can clear while the user signs in (the page sets and rotates
 * cookies). They are reported but do not end the wait; if one is still there at the time limit,
 * it is the reported cause.
 */
export const SOFT_LOGIN_CODES: ReadonlySet<string> = new Set([
  'youtube_session_ambiguous_cookie',
  'youtube_session_invalid_cookie',
]);

const MESSAGES: Record<string, string> = {
  youtube_login_window_closed: 'A janela de login foi fechada antes de concluir. Tente conectar de novo.',
  youtube_login_timeout: 'Tempo limite aguardando o login no YouTube Music.',
  youtube_login_build_failed: 'Não foi possível abrir a janela de login do YouTube Music.',
  youtube_login_navigation_failed: 'Não foi possível abrir a página de login do YouTube Music.',
  youtube_session_invalid_cookie: 'O navegador de login devolveu um cookie inválido. Tente conectar de novo.',
  youtube_session_ambiguous_cookie: 'O navegador de login devolveu cookies conflitantes. Tente conectar de novo.',
};

export function loginErrorMessage(code: string): string {
  return MESSAGES[code] ?? `Não foi possível concluir o login no YouTube Music (${code}).`;
}

/** Native commands reject with a plain string code (or an Error carrying it). */
export function loginErrorCode(error: unknown): string {
  const raw = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  const match = /youtube_[a-z_]+/.exec(raw);
  if (match) return match[0];
  // The window-closed command used to answer with a free-text message.
  if (/janela de login/i.test(raw)) return 'youtube_login_window_closed';
  return 'youtube_login_unknown';
}

export interface WaitForLoginOptions {
  intervalMs?: number;
  /** Delay before the first check, so a window that is still loading is not polled. */
  initialDelayMs?: number;
  timeoutMs?: number;
  /** Consecutive "not ready" errors tolerated before giving up. */
  maxTransient?: number;
  /** Emit a `waiting` event this often. */
  progressEverySeconds?: number;
  sleep?: (ms: number) => Promise<void>;
  onEvent?: (event: LoginEvent) => void;
}

/**
 * Polls `capture` until it reports a session. `false` means "not signed in yet": keep waiting.
 * A short run of "not ready" errors is expected while the window loads and is retried; a closed
 * window or an unrelated error ends the wait at once with a message that names the cause.
 */
export async function waitForLogin(capture: () => Promise<boolean>, options: WaitForLoginOptions = {}): Promise<void> {
  const interval = options.intervalMs ?? 1000;
  const timeout = options.timeoutMs ?? 300_000;
  const maxTransient = options.maxTransient ?? 8;
  const progressEvery = options.progressEverySeconds ?? 30;
  const sleep = options.sleep ?? ((ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms)));
  const emit = options.onEvent ?? (() => undefined);
  const maxAttempts = Math.max(1, Math.ceil(timeout / interval));

  await sleep(options.initialDelayMs ?? 1500);
  let streak = 0;
  let softStreak = 0;
  let lastSoft: string | null = null;
  for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
    const elapsedSeconds = Math.round((attempt * interval) / 1000);
    try {
      if (await capture()) {
        emit({ kind: 'connected', attempts: attempt, elapsedSeconds });
        return;
      }
      streak = 0;
      softStreak = 0;
      lastSoft = null;
    } catch (error) {
      const code = loginErrorCode(error);
      if (SOFT_LOGIN_CODES.has(code)) {
        softStreak += 1;
        lastSoft = code;
        // First occurrence and then once per progress period, not every second.
        if (softStreak === 1 || (progressEvery > 0 && softStreak % progressEvery === 0)) emit({ kind: 'transient', code, streak: softStreak });
      } else if (!TRANSIENT_LOGIN_CODES.has(code) || streak + 1 > maxTransient) {
        emit({ kind: 'failed', code });
        throw new YouTubeLoginError(code);
      } else {
        streak += 1;
        emit({ kind: 'transient', code, streak });
      }
    }
    if (progressEvery > 0 && elapsedSeconds % progressEvery === 0) emit({ kind: 'waiting', elapsedSeconds });
    await sleep(interval);
  }
  const code = lastSoft ?? 'youtube_login_timeout';
  emit({ kind: 'failed', code });
  throw new YouTubeLoginError(code);
}

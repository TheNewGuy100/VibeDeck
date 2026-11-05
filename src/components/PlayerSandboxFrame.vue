<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';

type SandboxStatus = 'loading' | 'success' | 'error';
type SandboxMessage =
  | { type: 'player-sandbox:play'; version: 1; videoId: string }
  | { type: 'player-sandbox:status'; version: 1; status: SandboxStatus; summary: string; enabled: boolean };

const props = defineProps<{
  connected: boolean;
  busy: boolean;
  status: string;
}>();

const emit = defineEmits<{
  play: [videoId: string];
}>();

const iframe = ref<HTMLIFrameElement | null>(null);
const VIDEO_ID_PATTERN = /^[A-Za-z0-9_-]{11}$/;
const SUMMARY_LIMIT = 160;

// The frame is intentionally self-contained: no network, parent DOM access, or Tauri bridge.
const sandboxDocument = `<!doctype html>
<html lang="pt-BR">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'none'; img-src 'none'; media-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'">
  <title>Player sandbox local</title>
  <style>
    :root { color-scheme: dark; font: 14px/1.45 system-ui, sans-serif; background: #141922; color: #f2f4f7; }
    * { box-sizing: border-box; }
    body { margin: 0; padding: 18px; }
    main { max-width: 560px; margin: 0 auto; }
    label { display: block; margin-bottom: 7px; color: #c5ccd6; font-weight: 650; }
    input { width: 100%; min-height: 42px; padding: 9px 12px; border: 1px solid #46505e; border-radius: 9px; outline: none; background: #202733; color: inherit; font: inherit; }
    input:focus-visible, button:focus-visible { outline: 2px solid #b9f36b; outline-offset: 3px; }
    button { min-height: 42px; margin-top: 11px; padding: 9px 16px; border: 0; border-radius: 9px; background: #b9f36b; color: #18210e; font: 700 14px/1.2 system-ui, sans-serif; cursor: pointer; }
    button:hover:not(:disabled) { background: #c9ff82; }
    button:disabled { cursor: not-allowed; opacity: .48; }
    #message { min-height: 21px; margin: 12px 0 0; color: #bac4d0; overflow-wrap: anywhere; }
    #message[data-kind="error"] { color: #ff9b9b; }
    @media (max-width: 420px) { body { padding: 14px; } button { width: 100%; } }
    @media (prefers-reduced-motion: reduce) { *, *::before, *::after { scroll-behavior: auto !important; } }
  </style>
</head>
<body>
  <main>
    <label for="video-id">ID do vídeo do YouTube (11 caracteres)</label>
    <input id="video-id" name="video-id" type="text" maxlength="11" minlength="11" pattern="[A-Za-z0-9_-]{11}" autocomplete="off" autocapitalize="off" spellcheck="false" aria-describedby="message">
    <button id="play" type="button" disabled>Solicitar reprodução</button>
    <p id="message" role="status" aria-live="polite">Aguardando conexão com o player.</p>
  </main>
  <script>
    (() => {
      const input = document.getElementById('video-id');
      const button = document.getElementById('play');
      const message = document.getElementById('message');
      const validId = /^[A-Za-z0-9_-]{11}$/;
      const suspiciousText = /(?:https?:\\/\\/|\\bwww\\.|[\\w.+-]+@[\\w.-]+\\.[A-Za-z]{2,}|(?:cookie|token|credential|authorization|signature|signed\\s*url|raw\\s*error))/i;
      const exactKeys = (value, keys) => value && typeof value === 'object' && !Array.isArray(value)
        && Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));

      window.addEventListener('message', (event) => {
        if (event.source !== window.parent) return;
        const data = event.data;
        if (!exactKeys(data, ['type', 'version', 'status', 'summary', 'enabled'])
          || data.type !== 'player-sandbox:status' || data.version !== 1
          || !['loading', 'success', 'error'].includes(data.status)
          || typeof data.summary !== 'string' || data.summary.length > 160
          || suspiciousText.test(data.summary) || typeof data.enabled !== 'boolean') return;

        button.disabled = !data.enabled;
        message.textContent = data.summary;
        message.dataset.kind = data.status;
      });

      button.addEventListener('click', () => {
        const videoId = input.value;
        if (button.disabled || !validId.test(videoId)) {
          message.textContent = 'Informe um ID válido com exatamente 11 caracteres.';
          message.dataset.kind = 'error';
          input.focus();
          return;
        }
        window.parent.postMessage({ type: 'player-sandbox:play', version: 1, videoId }, '*');
      });
    })();
  </scr${'ipt'}>
</body>
</html>`;

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, keys: string[]): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}

function safeSummary(value: string): string {
  const normalized = value.replace(/[\u0000-\u001f\u007f]/g, ' ').trim();
  if (!normalized || /(?:https?:\/\/|\bwww\.|[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}|(?:cookie|token|credential|authorization|signature|signed\s*url|raw\s*error))/i.test(normalized)) {
    return 'O status do player foi ocultado por segurança.';
  }
  return normalized.slice(0, SUMMARY_LIMIT);
}

function currentStatus(): Extract<SandboxMessage, { type: 'player-sandbox:status' }> {
  const status: SandboxStatus = props.busy
    ? 'loading'
    : /\b(erro|error|falh|indispon[ií]vel|desconectad|não foi possível|não iniciou)/i.test(props.status)
      ? 'error'
      : 'success';
  const summary = !props.connected
      ? 'Player nativo desconectado.'
      : props.busy
        ? 'Solicitação de reprodução em andamento.'
        : status === 'error'
        ? safeSummary(props.status || 'Falha no player nativo.')
        : safeSummary(props.status || 'Player pronto para receber uma solicitação manual.');

  return {
    type: 'player-sandbox:status',
    version: 1,
    status: !props.connected ? 'error' : status,
    summary,
    enabled: props.connected && !props.busy,
  };
}

function sendCurrentStatus() {
  const target = iframe.value?.contentWindow;
  if (!target) return;
  // O targetOrigin '*' é necessário porque sandbox sem allow-same-origin tem origem opaca;
  // o destino segue sendo exclusivamente o WindowProxy deste iframe.
  target.postMessage(currentStatus(), '*');
}

function handleFrameMessage(event: MessageEvent<unknown>) {
  if (event.source !== iframe.value?.contentWindow || !isPlainRecord(event.data)) return;
  const data = event.data;
  if (!hasExactKeys(data, ['type', 'version', 'videoId'])
    || data.type !== 'player-sandbox:play'
    || data.version !== 1
    || typeof data.videoId !== 'string'
    || !VIDEO_ID_PATTERN.test(data.videoId)
    || !props.connected
    || props.busy) return;

  emit('play', data.videoId);
}

function handleFrameLoad() {
  sendCurrentStatus();
}

onMounted(() => window.addEventListener('message', handleFrameMessage));
onBeforeUnmount(() => window.removeEventListener('message', handleFrameMessage));
watch(() => [props.connected, props.busy, props.status] as const, sendCurrentStatus);
</script>

<template>
  <section class="player-sandbox" aria-labelledby="player-sandbox-title">
    <header class="player-sandbox-header">
      <div>
        <p class="player-sandbox-eyebrow">FERRAMENTA DE DESENVOLVIMENTO</p>
        <h2 id="player-sandbox-title">Player sandbox</h2>
      </div>
      <span class="player-sandbox-badge">Isolado</span>
    </header>
    <p class="player-sandbox-description">
      O iframe envia somente um ID validado após ação manual. O VibeDeck resolve o stream pela sessão já conectada; o iframe não acessa cookies nem a sessão.
    </p>
    <p class="player-sandbox-warning" role="note">
      Ao iniciar, o teste substitui a faixa e a fila ativas no VibeDeck e pode emitir áudio. Nada começa automaticamente.
    </p>
    <iframe
      ref="iframe"
      class="player-sandbox-frame"
      title="Formulário isolado para solicitar reprodução manual"
      sandbox="allow-scripts"
      :srcdoc="sandboxDocument"
      @load="handleFrameLoad"
    />
  </section>
</template>

<style scoped>
.player-sandbox {
  width: 100%;
  min-width: 0;
  padding: clamp(14px, 2.5vw, 20px);
  border: 1px solid var(--border-color, #343d49);
  border-radius: 16px;
  color: var(--text-primary, #f2f4f7);
  background: var(--surface-secondary, #1b222c);
}

.player-sandbox-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.player-sandbox-eyebrow {
  margin: 0 0 5px;
  color: var(--accent-color, #b9f36b);
  font-size: 10px;
  font-weight: 750;
  letter-spacing: 1.1px;
}

h2 {
  margin: 0;
  font: 600 clamp(17px, 3vw, 21px)/1.2 'Space Grotesk', system-ui, sans-serif;
}

.player-sandbox-badge {
  flex: 0 0 auto;
  padding: 4px 9px;
  border: 1px solid color-mix(in srgb, var(--accent-color, #b9f36b) 40%, transparent);
  border-radius: 999px;
  color: var(--accent-color, #b9f36b);
  font-size: 11px;
  font-weight: 650;
}

.player-sandbox-description {
  max-width: 68ch;
  margin: 10px 0 14px;
  color: var(--text-secondary, #aeb7c4);
  font-size: 13px;
}

.player-sandbox-warning {
  margin: 0 0 12px;
  padding: 9px 11px;
  border-left: 3px solid #e9b95f;
  border-radius: 4px 8px 8px 4px;
  color: var(--text-secondary, #aeb7c4);
  background: color-mix(in srgb, #e9b95f 9%, transparent);
  font-size: 12px;
}

.player-sandbox-frame {
  display: block;
  width: 100%;
  min-height: 186px;
  border: 1px solid var(--border-color, #343d49);
  border-radius: 11px;
  background: #141922;
}

@media (max-width: 480px) {
  .player-sandbox { padding: 13px; }
  .player-sandbox-description { font-size: 12px; }
  .player-sandbox-warning { font-size: 11px; }
  .player-sandbox-frame { min-height: 205px; }
}

:global(.light-theme) .player-sandbox {
  color: var(--text-primary, #20252e);
  background: var(--surface-secondary, #f1f4f6);
}

:global(.light-theme) .player-sandbox-description {
  color: var(--text-secondary, #5d6875);
}

:global(.light-theme) .player-sandbox-warning {
  color: var(--text-secondary, #5d6875);
}

:global(.player-sandbox :focus-visible) {
  outline: 2px solid var(--accent-color, #b9f36b);
  outline-offset: 3px;
}
</style>

import type { NativeAudioPlaybackDiagnostic } from '../interfaces/player';

const SAFE_ERROR_CODES = new Set([
  'http_400_ump',
  'http_403',
  'http_error',
  'invalid_response',
  'ump_decode_error',
  'output_device_error',
  'output_start_error',
  'container_probe_error',
  'codec_error',
  'cancelled',
  'network_error',
]);

export function formatSandboxAudioDiagnostic(
  diagnostic: NativeAudioPlaybackDiagnostic,
  expectedPlayerActivationSequence: number,
): string {
  if (diagnostic.playerActivationSequence !== expectedPlayerActivationSequence) {
    return 'Falha nativa; o diagnóstico disponível pertence a outra ativação.';
  }

  const errorCode = diagnostic.errorCode && SAFE_ERROR_CODES.has(diagnostic.errorCode)
    ? diagnostic.errorCode
    : 'unknown_error';
  const stage = /^[a-z0-9_]{1,32}$/i.test(diagnostic.stage) ? diagnostic.stage : 'unknown';
  const mediaBytes = Number.isSafeInteger(diagnostic.mediaBytes) && diagnostic.mediaBytes >= 0
    ? diagnostic.mediaBytes
    : 0;

  return `Falha nativa: código=${errorCode}; etapa=${stage}; decoder=${diagnostic.decoderStarted ? 'sim' : 'não'}; saída=${diagnostic.outputStarted ? 'sim' : 'não'}; mídia=${mediaBytes} bytes.`;
}

export function formatSandboxPlaybackFailure(error: unknown): string {
  const message = error instanceof Error
    ? error.message
    : typeof error === 'string' ? error : '';
  const normalized = message.trim().toLowerCase();
  let code = 'native_playback_failed';
  let stage = 'activation';

  if (normalized === 'capture_setup_failed') {
    code = 'capture_setup_failed';
    stage = 'capture_setup';
  } else if (normalized === 'no_accepted_ump_response') {
    code = 'no_accepted_ump_response';
    stage = 'response_capture';
  } else if (normalized === 'ump_body_capture_failed') {
    code = 'ump_body_capture_failed';
    stage = 'body_capture';
  } else if (normalized === 'ump_response_validation_failed' || normalized === 'ump_response_body_invalid') {
    code = 'ump_response_invalid';
    stage = 'response_validation';
  } else if (normalized === 'ump_audio_handoff_failed') {
    code = 'audio_handoff_failed';
    stage = 'audio_handoff';
  } else if (normalized === 'audio_start_task_failed') {
    code = 'audio_start_failed';
    stage = 'audio_startup';
  } else if (/tempo limite|timed out|timeout/i.test(normalized)) {
    code = 'stream_resolution_timeout';
    stage = 'stream_resolution';
  } else if (/youtube_session_required|faça login|login/i.test(normalized)) {
    code = 'youtube_session_required';
    stage = 'session_check';
  } else if (/output device|audio device|no default audio output/i.test(normalized)) {
    code = 'output_device_error';
    stage = 'audio_output';
  } else if (/decoder|codec|demux|container|probe/i.test(normalized)) {
    code = 'audio_decode_error';
    stage = 'decode';
  } else if (/cancel|abort/i.test(normalized)) {
    code = 'cancelled';
    stage = 'activation';
  }

  return `Falha nativa: código=${code}; etapa=${stage}.`;
}

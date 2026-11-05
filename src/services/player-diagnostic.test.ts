import { describe, expect, it } from 'vitest';
import type { NativeAudioPlaybackDiagnostic } from '../interfaces/player';
import { formatSandboxAudioDiagnostic, formatSandboxPlaybackFailure } from './player-diagnostic';

const diagnostic: NativeAudioPlaybackDiagnostic = {
  activationSequence: 4,
  playerActivationSequence: 17,
  stage: 'failed',
  outcome: 'error',
  httpStatus: null,
  responseBytes: 924947,
  mediaBytes: 732198,
  umpPartKinds: [{ kind: 21, count: 4 }],
  decoderStarted: false,
  outputStarted: false,
  errorCode: 'codec_error',
  updatedAtUnixMs: 1,
};

describe('formatSandboxAudioDiagnostic', () => {
  it('forwards the matching native failure code and safe pipeline milestones', () => {
    expect(formatSandboxAudioDiagnostic(diagnostic, 17)).toBe(
      'Falha nativa: código=codec_error; etapa=failed; decoder=não; saída=não; mídia=732198 bytes.',
    );
  });

  it('does not forward a diagnostic belonging to a different activation', () => {
    expect(formatSandboxAudioDiagnostic(diagnostic, 18)).toBe(
      'Falha nativa; o diagnóstico disponível pertence a outra ativação.',
    );
  });

  it('replaces unknown error codes and unsafe stage text with fixed values', () => {
    expect(formatSandboxAudioDiagnostic({
      ...diagnostic,
      stage: 'https://private.invalid/signed?token=secret',
      errorCode: 'raw signed URL: https://private.invalid',
    }, 17)).toBe(
      'Falha nativa: código=unknown_error; etapa=unknown; decoder=não; saída=não; mídia=732198 bytes.',
    );
  });
});

describe('formatSandboxPlaybackFailure', () => {
  it('maps resolver timeouts to a fixed safe category', () => {
    expect(formatSandboxPlaybackFailure(new Error('Tempo limite ao resolver o stream de áudio do YouTube Music.')))
      .toBe('Falha nativa: código=stream_resolution_timeout; etapa=stream_resolution.');
  });

  it('maps fixed bridge errors and never forwards raw error text', () => {
    expect(formatSandboxPlaybackFailure(new Error('no_accepted_ump_response')))
      .toBe('Falha nativa: código=no_accepted_ump_response; etapa=response_capture.');
    expect(formatSandboxPlaybackFailure(new Error('decoder error at https://private.invalid/?token=secret')))
      .toBe('Falha nativa: código=audio_decode_error; etapa=decode.');
  });
});

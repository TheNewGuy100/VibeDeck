export interface MascotAudioBands {
  bassMinHz: number;
  bassMaxHz: number;
  voiceMinHz: number;
  voiceMaxHz: number;
  energyMinHz: number;
  energyMaxHz: number;
  sampleRate?: number;
}

export interface MascotAudioResponse {
  bass: number;
  beat: number;
  depth: number;
  voice: number;
  energy: number;
}

export interface MascotAudioResponseState extends MascotAudioResponse {
  previousBass: number;
  hasBassSample: boolean;
  lastTimeMs: number | undefined;
}

export const MASCOT_ARC_ROLES = ['bass', 'voice', 'energy'] as const;
export type MascotArcRole = typeof MASCOT_ARC_ROLES[number];

// Arc identity is semantic, not amplitude-based. Keeping this mapping explicit
// prevents close responses from changing the arc's color or position.
export function getMascotArcResponses(response: MascotAudioResponse) {
  return MASCOT_ARC_ROLES.map((role) => response[role]);
}

// The native analyzer publishes after each 1024-sample hop (about 21-23 ms at
// common output rates). The UI polls at 33 ms; this deadband filters tiny
// positive bass deltas while the short attack and longer release keep genuine
// beats immediate without making shared depth shimmer between samples.
const BASS_TRANSIENT_DEADBAND = 0.04;
const BASS_ATTACK_MS = 36;
const BASS_RELEASE_MS = 300;
const BEAT_ATTACK_MS = 14;
const BEAT_RELEASE_MS = 180;
const VOICE_ATTACK_MS = 85;
const VOICE_RELEASE_MS = 380;
const ENERGY_ATTACK_MS = 105;
const ENERGY_RELEASE_MS = 440;

export function createMascotAudioResponseState(): MascotAudioResponseState {
  return { bass: 0, beat: 0, depth: 0, voice: 0, energy: 0, previousBass: 0, hasBassSample: false, lastTimeMs: undefined };
}

function smooth(current: number, target: number, deltaMs: number, attackMs: number, releaseMs: number) {
  const timeConstant = target > current ? attackMs : releaseMs;
  const factor = 1 - Math.exp(-deltaMs / timeConstant);
  return current + (target - current) * factor;
}

function bandAverage(levels: number[], lowHz: number, highHz: number, sampleRate: number) {
  if (!levels.length) return 0;
  const nyquist = sampleRate / 2;
  const toIndex = (frequency: number) => Math.floor((Math.log(Math.max(1, Math.min(nyquist, frequency))) / Math.log(nyquist)) * levels.length);
  const start = Math.min(levels.length - 1, Math.max(0, toIndex(lowHz)));
  const end = Math.min(levels.length, Math.max(start + 1, toIndex(highHz)));
  let total = 0;
  for (let index = start; index < end; index += 1) total += Math.max(0, Math.min(1, levels[index] ?? 0));
  return total / (end - start);
}

export function updateMascotAudioResponse(
  state: MascotAudioResponseState,
  levels: number[],
  timeMs: number,
  bands: MascotAudioBands,
): MascotAudioResponse {
  const deltaMs = state.lastTimeMs === undefined ? 16.67 : Math.max(0, timeMs - state.lastTimeMs);
  state.lastTimeMs = timeMs;
  if (!levels.length) {
    state.bass = 0;
    state.beat = 0;
    state.depth = 0;
    state.voice = 0;
    state.energy = 0;
    state.previousBass = 0;
    state.hasBassSample = false;
    return { bass: 0, beat: 0, depth: 0, voice: 0, energy: 0 };
  }

  const sampleRate = bands.sampleRate ?? 48000;
  const bassLevel = bandAverage(levels, bands.bassMinHz, bands.bassMaxHz, sampleRate);
  const voiceLevel = bandAverage(levels, bands.voiceMinHz, bands.voiceMaxHz, sampleRate);
  const energyLevel = bandAverage(levels, bands.energyMinHz, bands.energyMaxHz, sampleRate);
  const transient = state.hasBassSample
    ? Math.max(0, bassLevel - state.previousBass - BASS_TRANSIENT_DEADBAND)
    : 0;
  state.previousBass = bassLevel;
  state.hasBassSample = true;
  const beatTarget = Math.min(1, transient * 3.2);

  state.bass = smooth(state.bass, bassLevel, deltaMs, BASS_ATTACK_MS, BASS_RELEASE_MS);
  state.beat = smooth(state.beat, beatTarget, deltaMs, BEAT_ATTACK_MS, BEAT_RELEASE_MS);
  state.depth = state.beat;
  state.voice = smooth(state.voice, voiceLevel, deltaMs, VOICE_ATTACK_MS, VOICE_RELEASE_MS);
  state.energy = smooth(state.energy, energyLevel, deltaMs, ENERGY_ATTACK_MS, ENERGY_RELEASE_MS);
  return { bass: state.bass, beat: state.beat, depth: state.depth, voice: state.voice, energy: state.energy };
}

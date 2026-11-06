/**
 * Phase and bass-reaction logic for the snake shader.
 *
 * Everything that moves (body clip, texture flow, bulge pulses) reads ONE phase in [0, 1).
 * Audio only changes how fast that phase advances and how much the body swells; it never
 * sets the phase directly. That is what keeps the loop closed while the bass speeds it up.
 */

/** Longest time step accepted per frame; a throttled tab must not make the phase jump. */
export const MAX_DT = 0.1;

export const wrap01 = (value: number): number => ((value % 1) + 1) % 1;

const clamp = (value: number, low: number, high: number): number =>
  Math.min(Math.max(value, low), high);

/** Phase integrator: phase += dt * speed / period, kept in [0, 1). */
export class PhaseClock {
  phase = 0;

  constructor(public periodSeconds = 6) {}

  /** Advances by `dt` seconds at a speed multiplier; negative speeds are treated as zero. */
  advance(dt: number, speed = 1): number {
    const step = clamp(Number.isFinite(dt) ? dt : 0, 0, MAX_DT);
    const rate = Math.max(0, Number.isFinite(speed) ? speed : 0);
    this.phase = wrap01(this.phase + (step * rate) / Math.max(0.1, this.periodSeconds));
    return this.phase;
  }
}

/** One-pole smoothing with a fast attack and a slow release, so a kick does not jerk the speed. */
export class BassEnvelope {
  value = 0;

  constructor(
    public attackSeconds = 0.04,
    public releaseSeconds = 0.28,
  ) {}

  update(raw: number, dt: number): number {
    const target = clamp(Number.isFinite(raw) ? raw : 0, 0, 1);
    const step = clamp(Number.isFinite(dt) ? dt : 0, 0, MAX_DT);
    const tau = target > this.value ? this.attackSeconds : this.releaseSeconds;
    const k = tau <= 0 ? 1 : 1 - Math.exp(-step / tau);
    this.value += (target - this.value) * k;
    return this.value;
  }
}

/** Speed multiplier from the bass envelope; bounded and never negative. */
export function speedFromBass(envelope: number, gain: number, min = 0.6, max = 2): number {
  return clamp(1 + gain * envelope, Math.max(0, min), max);
}

/** Body swell (fraction of the radius) from the bass envelope, capped by `limit`. */
export function swellFromBass(envelope: number, gain: number, limit: number): number {
  return clamp(envelope * gain, 0, Math.max(0, limit));
}

export interface Pulse {
  /** Position along the body: 0 at the head, 1 at the tail. */
  position: number;
  amplitude: number;
}

/** Bulges that start at the head on a beat and travel to the tail. */
export class PulseTrain {
  readonly pulses: Pulse[] = [];

  constructor(
    public maxPulses = 4,
    public travelSeconds = 1.4,
  ) {}

  spawn(amplitude: number): void {
    if (this.pulses.length >= this.maxPulses) this.pulses.shift();
    this.pulses.push({ position: 0, amplitude: clamp(amplitude, 0, 1) });
  }

  update(dt: number, speed = 1): void {
    const step = clamp(Number.isFinite(dt) ? dt : 0, 0, MAX_DT);
    const rate = Math.max(0, speed) / Math.max(0.1, this.travelSeconds);
    for (const pulse of this.pulses) pulse.position += step * rate;
    for (let i = this.pulses.length - 1; i >= 0; i -= 1) {
      if (this.pulses[i].position > 1.15) this.pulses.splice(i, 1);
    }
  }
}

/** Largest swell that keeps the swollen tube clear of its own bends. */
export function safeSwellLimit(radius: number, minBendRadius: number, margin = 0.6): number {
  if (!(radius > 0) || !(minBendRadius > 0)) return 0;
  // The tube may grow until it reaches `margin` of the tightest bend's radius.
  return Math.max(0, (minBendRadius * margin - radius) / radius);
}

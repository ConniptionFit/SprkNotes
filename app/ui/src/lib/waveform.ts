// The source buttons' sound meter: one smooth line whose swell follows the voice, flat in silence.
// Pure math so it is cheap, testable and portable to other front ends (Android draws the same
// numbers on its own canvas); the window draws it on a small canvas at whole device pixels.
//
// Levels arrive about twenty times a second, but the window only hears about a level that differs
// from the last one: during silence the same 0 repeats. So the meter holds the newest level and
// keeps easing toward it on display frames until it lies flat: quickly up (attack) and slower
// down (release). Below the gate (room hiss, a muted microphone) the level counts as silence, so
// dead air settles into a flat line instead of a faint wobble.
//
// Nine bars, the earlier style, is archived: see docs/archive/nine-bars.md for where its code is.

/** Levels below this (about -39 dBFS peak on the 0 to 1 meter scale) are silence. */
export const GATE = 0.35;
/** Easing time constants in milliseconds: rise fast, fall gently. */
export const ATTACK_MS = 45;
export const RELEASE_MS = 170;
/** Closer than this counts as arrived, so drawing can stop. */
const EPS = 0.004;

/** A meter level with the noise gate applied and the rest stretched back to 0..1. */
export function gated(level: number): number {
  const l = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
  return l <= GATE ? 0 : (l - GATE) / (1 - GATE);
}

/** Move `current` toward `target` for `dtMs` of time: attack when rising, release when falling. */
export function ease(current: number, target: number, dtMs: number): number {
  const dt = Math.max(0, Math.min(Number.isFinite(dtMs) ? dtMs : 0, 250));
  const k = 1 - Math.exp(-dt / (target > current ? ATTACK_MS : RELEASE_MS));
  return current + (target - current) * k;
}

/** One meter between frames: the line's eased loudness and its drift phase in milliseconds. */
export type Meter = { env: number; phase: number };

/** A silent, still meter. */
export function meterStart(): Meter {
  return { env: 0, phase: 0 };
}

/**
 * Advance by `dtMs` toward the newest level, whether or not it changed. With `snap` (reduced
 * motion) the loudness jumps straight to the level and the line does not drift.
 */
export function meterStep(m: Meter, level: number, dtMs: number, snap = false): Meter {
  const dt = Math.max(0, Math.min(Number.isFinite(dtMs) ? dtMs : 0, 250));
  const now = gated(level);
  const env = snap ? now : ease(m.env, now, dt);
  // The line drifts only while it has a swell to show; in silence it lies still.
  const phase = snap || env < EPS ? m.phase : (m.phase + dt) % 600_000;
  return { env, phase };
}

/** Nothing left to move: silent now and the line already flat. */
export function meterStill(m: Meter, level: number): boolean {
  return gated(level) === 0 && m.env < EPS;
}

/** A still meter drawn exactly flat, so the resting line never sits a hair off. */
export function meterRest(m: Meter): Meter {
  return { ...m, env: 0 };
}

/**
 * The line: y offsets (positive is up, as a fraction of half the height) at `points` evenly spaced
 * x positions. A few slow sines make a soft, organic swell; it tapers to the ends and is exactly
 * flat when `env` is 0.
 */
export function linePoints(env: number, phaseMs: number, points: number): number[] {
  const a = Math.max(0, Math.min(1, Number.isFinite(env) ? env : 0)) * 0.84;
  const t = (Number.isFinite(phaseMs) ? phaseMs : 0) / 1000;
  const n = Math.max(2, Math.floor(points));
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    const u = i / (n - 1);
    const taper = Math.sin(u * Math.PI);
    const y = Math.sin(u * 9 + t * 7) * 0.6 + Math.sin(u * 17 - t * 11) * 0.3 + Math.sin(u * 4 + t * 3) * 0.25;
    out.push(a === 0 ? 0 : y * a * taper);
  }
  return out;
}

// The source buttons' sound meter, in two styles picked in Settings. "bars" (the default): nine
// bars, newest level in the middle and older ones moving outward, shaped like the classic "audio
// lines" icon. "line": one smooth stroke whose swell follows the voice. Both are flat in silence.
// Pure math so it is cheap, testable and portable to other front ends (Android draws the same
// numbers on its own canvas); the window draws them on a small canvas, snapped to whole device
// pixels so the bars and gaps stay even at every display scale.
//
// Levels arrive about twenty times a second, but the window only hears about a level that differs
// from the last one: during silence the same 0 repeats. So the meter samples the newest level on
// its own clock (SAMPLE_MS) and keeps going until everything has settled. Drawing the samples as
// they come looks jumpy, so the drawn values ease toward them on every display frame: quickly up
// (attack) and slower down (release). Below the gate (room hiss, a muted microphone) the level
// counts as silence, so dead air settles into a flat line instead of resting a little raised.

export const BARS = 9;
/** History slots: the middle bar shows the newest level, each step outward an older one. */
export const SLOTS = (BARS + 1) / 2;
/** Height of a silent bar, as a fraction of full height: a flat line. */
export const REST = 0.08;
/** Levels below this (about -39 dBFS peak on the 0 to 1 meter scale) are silence. */
export const GATE = 0.35;
/** Middle bars reach higher than the outer ones, as in the icon. */
const SHAPE = [0.45, 0.6, 0.75, 0.9, 1, 0.9, 0.75, 0.6, 0.45];
/** Which history slot each bar shows: newest (0) in the middle, older toward the edges. */
const SLOT = [4, 3, 2, 1, 0, 1, 2, 3, 4];
/** Easing time constants in milliseconds: rise fast, fall gently. */
export const ATTACK_MS = 45;
export const RELEASE_MS = 170;
/** The meter's own sampling clock, matching the app's twenty levels a second. */
export const SAMPLE_MS = 50;
/** Closer than this counts as arrived, so drawing can stop. */
const EPS = 0.004;

/** The two sound meter styles; "bars" is the default. */
export type WaveStyle = "bars" | "line";
export const WAVE_STYLES: readonly WaveStyle[] = ["bars", "line"];

/** A meter level with the noise gate applied and the rest stretched back to 0..1. */
export function gated(level: number): number {
  const l = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
  return l <= GATE ? 0 : (l - GATE) / (1 - GATE);
}

/** Newest first, at most SLOTS long. Input is clamped and gated. */
export function pushLevel(history: number[], level: number): number[] {
  return [gated(level), ...history].slice(0, SLOTS);
}

/** Target bar heights, REST..1, left to right. */
export function waveform(history: number[]): number[] {
  return SHAPE.map((w, i) => REST + (1 - REST) * (history[SLOT[i]] ?? 0) * w);
}

/** Silent bars: a flat line. */
export function flat(): number[] {
  return new Array(BARS).fill(REST);
}

/** Move each value toward its target for `dtMs` of time: attack when rising, release when falling. */
export function ease(current: number[], target: number[], dtMs: number): number[] {
  const dt = Math.max(0, Math.min(dtMs, 250));
  return target.map((t, i) => {
    const c = current[i] ?? REST;
    const k = 1 - Math.exp(-dt / (t > c ? ATTACK_MS : RELEASE_MS));
    return c + (t - c) * k;
  });
}

/** True once every value is close enough to its target to stop drawing frames. */
export function settled(current: number[], target: number[]): boolean {
  return target.every((t, i) => Math.abs((current[i] ?? REST) - t) < EPS);
}

/**
 * Everything one meter needs between frames: the sampled history (newest first), time since the
 * last sample, the drawn bar heights, the line's eased loudness and its phase in milliseconds.
 */
export type Meter = { hist: number[]; acc: number; bars: number[]; env: number; phase: number };

/** A silent, still meter. */
export function meterStart(): Meter {
  return { hist: new Array(SLOTS).fill(0), acc: 0, bars: flat(), env: 0, phase: 0 };
}

/**
 * Advance by `dtMs`. The newest level is sampled every SAMPLE_MS whether or not it changed, so a
 * silence that repeats the same 0 still walks the older bars down to the flat line. With `snap`
 * (reduced motion) the drawn values jump to their targets and the line does not drift.
 */
export function meterStep(m: Meter, level: number, dtMs: number, snap = false): Meter {
  const dt = Math.max(0, Math.min(Number.isFinite(dtMs) ? dtMs : 0, 250));
  let hist = m.hist;
  let acc = m.acc + dt;
  while (acc >= SAMPLE_MS) {
    acc -= SAMPLE_MS;
    hist = pushLevel(hist, level);
  }
  const target = waveform(hist);
  const now = gated(level);
  const bars = snap ? target : ease(m.bars, target, dt);
  const env = snap ? now : (ease([m.env], [now], dt)[0] ?? 0);
  // The line drifts only while it has a swell to show; in silence it lies still.
  const phase = snap || env < EPS ? m.phase : (m.phase + dt) % 600_000;
  return { hist, acc, bars, env, phase };
}

/** Nothing left to move: silent now, every sample silent and every drawn value at rest. */
export function meterStill(m: Meter, level: number): boolean {
  return gated(level) === 0 && m.hist.every((h) => h === 0) && settled(m.bars, flat()) && m.env < EPS;
}

/** A still meter drawn exactly flat, so the resting line never sits a hair off. */
export function meterRest(m: Meter): Meter {
  return { ...m, acc: 0, bars: flat(), env: 0 };
}

/**
 * Where the nine bars go on a canvas `width` by `height` device pixels, at `dpr` device pixels per
 * CSS pixel. Every bar and every gap is a whole number of device pixels and the same as the others,
 * so the spacing looks even at 100, 125, 150 or 200 percent. Each bar is centered on a whole pixel
 * row too; a silent bar is a small dot about as wide as the bar.
 */
export function barRects(
  heights: number[],
  width: number,
  height: number,
  dpr: number,
): { x: number; y: number; w: number; h: number }[] {
  const scale = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  const bw = Math.max(1, Math.round(2 * scale));
  const gap = Math.max(1, Math.round(1.5 * scale));
  const n = heights.length;
  const total = n * bw + (n - 1) * gap;
  const left = Math.max(0, Math.floor((width - total) / 2));
  const room = Math.max(bw, Math.floor(height * 0.92));
  return heights.map((v, i) => {
    const frac = (Number.isFinite(v) ? Math.max(REST, Math.min(1, v)) : REST) - REST;
    let h = Math.max(bw, Math.round(bw + (room - bw) * (frac / (1 - REST))));
    // Same parity as the canvas so the bar centers on a whole pixel row and never blurs.
    if ((height - h) % 2 !== 0) h += h < room ? 1 : -1;
    return { x: left + i * (bw + gap), y: (height - h) / 2, w: bw, h };
  });
}

/**
 * The line style: y offsets (positive is up, as a fraction of half the height) at `points` evenly
 * spaced x positions. A few slow sines make a soft, organic swell; it tapers to the ends and is
 * exactly flat when `env` is 0.
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

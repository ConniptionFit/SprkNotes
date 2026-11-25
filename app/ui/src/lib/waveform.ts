// The source buttons' level waveform, in two styles picked in Settings. "bars" (the default): nine
// bars, newest level in the middle and older ones moving outward, shaped like the classic "audio
// lines" icon. "rolling": the same nine bars sliding right to left like a voice note, newest at the
// right edge. Pure math so it is cheap, testable and
// portable to other front ends; the bars are drawn with a CSS transform only.
//
// Levels arrive about twenty times a second. Drawing them as they come looks jumpy, so each bar
// eases toward its target on every display frame: quickly up (attack) and slower down (release).
// Below the gate (room hiss, a muted microphone) the level counts as silence, so dead air settles
// into a flat line instead of resting a little raised.

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

/** Move each bar toward its target for `dtMs` of time: attack when rising, release when falling. */
export function ease(current: number[], target: number[], dtMs: number): number[] {
  const dt = Math.max(0, Math.min(dtMs, 250));
  return target.map((t, i) => {
    const c = current[i] ?? REST;
    const k = 1 - Math.exp(-dt / (t > c ? ATTACK_MS : RELEASE_MS));
    return c + (t - c) * k;
  });
}

/** True once every bar is close enough to its target to stop drawing frames. */
export function settled(current: number[], target: number[]): boolean {
  return target.every((t, i) => Math.abs((current[i] ?? REST) - t) < 0.004);
}

/** The two waveform styles; "bars" is the default. */
export type WaveStyle = "bars" | "rolling";
export const WAVE_STYLES: readonly WaveStyle[] = ["bars", "rolling"];

/** Rolling bars: a new column enters every step, and the row slides smoothly between steps. */
export const ROLL_STEP_MS = 70;
/** Rolling state: an eased loudness, time since the last column, and columns newest first. */
export type Roll = { env: number; acc: number; cols: number[] };

/** A silent rolling row: one extra column so a bar can slide in while another slides out. */
export function rollStart(): Roll {
  return { env: 0, acc: 0, cols: new Array(BARS + 1).fill(0) };
}

/** Add a column now (reduced motion uses this on every level instead of sliding). */
export function rollPush(r: Roll, value: number): Roll {
  const v = Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
  return { env: v, acc: 0, cols: [v, ...r.cols].slice(0, BARS + 1) };
}

/** Advance by one display frame: ease toward the newest gated level and add columns on schedule. */
export function rollStep(r: Roll, level: number, dtMs: number): Roll {
  const dt = Math.max(0, Math.min(dtMs, 250));
  const env = ease([r.env], [gated(level)], dt)[0] ?? 0;
  let acc = r.acc + dt;
  let cols = r.cols;
  while (acc >= ROLL_STEP_MS) {
    acc -= ROLL_STEP_MS;
    cols = [env, ...cols].slice(0, BARS + 1);
  }
  return { env, acc, cols };
}

/** Bar heights left to right (oldest first), each from REST (silent) to 1. */
export function rollHeights(r: Roll): number[] {
  return [...r.cols].reverse().map((v) => REST + (1 - REST) * v);
}

/** How far the row still sits to the right, in steps (1 just after a column enters, then toward 0). */
export function rollShift(r: Roll): number {
  return 1 - Math.min(1, r.acc / ROLL_STEP_MS);
}

/** Nothing left to move: silent now and every visible column flat. */
export function rollStill(r: Roll): boolean {
  return r.env < 0.004 && r.cols.every((c) => c < 0.004);
}

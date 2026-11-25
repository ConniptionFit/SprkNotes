// The source buttons' level waveform: five bars, newest level in the middle and older ones moving
// outward, shaped like the classic "audio lines" icon. Pure math so it is cheap and testable; the
// bars are drawn with a CSS transform only.

export const BARS = 5;
/** Smallest bar, so a silent source still shows five short bars instead of nothing. */
export const FLOOR = 0.18;
/** Middle bars reach higher than the outer ones, as in the icon. */
const SHAPE = [0.6, 0.85, 1, 0.85, 0.6];
/** Which history slot each bar shows: newest (0) in the middle, older toward the edges. */
const SLOT = [2, 1, 0, 1, 2];

/** Newest first, at most BARS long. Out-of-range input is clamped to 0..1. */
export function pushLevel(history: number[], level: number): number[] {
  const l = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
  return [l, ...history].slice(0, BARS);
}

/** Bar heights, FLOOR..1, left to right. */
export function waveform(history: number[]): number[] {
  return SHAPE.map((w, i) => {
    const l = history[SLOT[i]] ?? 0;
    return FLOOR + (1 - FLOOR) * l * w;
  });
}

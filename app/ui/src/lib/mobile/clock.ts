// The session clock on the phone's control bar: time spent transcribing, not counting pauses.
// Pure, so it is testable without a window.

export type Clock = { before: number; since: number | null };

export const CLOCK_START: Clock = { before: 0, since: null };

/** Running from `now` (Start or Resume). */
export function run(c: Clock, now: number): Clock {
  return c.since === null ? { ...c, since: now } : c;
}

/** Stopped at `now` (Pause), keeping the time so far. */
export function hold(c: Clock, now: number): Clock {
  return c.since === null ? c : { before: c.before + Math.max(0, now - c.since), since: null };
}

/** Milliseconds counted at `now`. */
export function elapsed(c: Clock, now: number): number {
  return c.before + (c.since === null ? 0 : Math.max(0, now - c.since));
}

/** 00:18:05 */
export function hms(ms: number): string {
  const s = Math.floor(Math.max(0, ms) / 1000);
  return [Math.floor(s / 3600), Math.floor((s % 3600) / 60), s % 60].map((v) => String(v).padStart(2, "0")).join(":");
}

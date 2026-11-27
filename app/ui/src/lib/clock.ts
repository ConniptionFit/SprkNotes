// Elapsed session time, shared by the status chip and the times on lines that are still settling.
// Transcript lines carry elapsed time as HH:MM:SS (SPEC 5.1); the chip shows the short form.

/** "00:12:41" to milliseconds; anything else is null. */
export function stampMs(stamp: string | undefined): number | null {
  const m = /^(\d+):(\d{2}):(\d{2})$/.exec(stamp ?? "");
  if (!m) return null;
  return ((Number(m[1]) * 60 + Number(m[2])) * 60 + Number(m[3])) * 1000;
}

/** Milliseconds as a transcript stamp: "00:12:41". */
export function stampText(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor(s / 60) % 60)}:${p(s % 60)}`;
}

/** Milliseconds for the status chip: "12:41", or "1:02:03" past an hour. */
export function clockText(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const mm = String(Math.floor(s / 60) % 60).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/**
 * Elapsed time that counts only while transcribing. Real time, not ticks, so a throttled window
 * does not slow it. A committed line's stamp moves it forward, so a continued transcript picks up
 * where its lines are instead of starting at zero.
 */
export class Elapsed {
  ms = 0;
  private since: number | null = null;

  /** Start counting (or keep counting) at `now`. */
  run(now: number): void {
    if (this.since === null) this.since = now;
  }

  /** Stop counting at `now`, keeping the time so far (paused). */
  hold(now: number): void {
    this.ms = this.at(now);
    this.since = null;
  }

  /** Back to zero, stopped. */
  reset(): void {
    this.ms = 0;
    this.since = null;
  }

  /** A committed line said it is at least this late. */
  seen(stamp: string, now: number): void {
    const ms = stampMs(stamp);
    if (ms === null || ms <= this.at(now)) return;
    this.ms = ms;
    if (this.since !== null) this.since = now;
  }

  at(now: number): number {
    return this.since === null ? this.ms : this.ms + Math.max(0, now - this.since);
  }
}

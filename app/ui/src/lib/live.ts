/**
 * Live view logic (ADR-008). Pure, so it is tested without a browser.
 *
 * - `Rows` holds sentences that locked in the live view. They sit below the committed lines
 *   until the committed line for the same speech arrives, then they are replaced silently.
 * - `gate` decides which tentative words are visible yet (a short wait lets the engine settle).
 * - `typed` reveals text a few characters at a time, faster when behind.
 */
import type { Caption } from "./types";

export type Row = { uid: number; utt: number; speaker: string; text: string; at: number };

/** Locked rows older than this are dropped, in case their committed line never arrives. */
export const ROW_TTL_MS = 45_000;

const isMe = (speaker: string) => speaker === "Me";

const normWords = (t: string) =>
  t
    .toLowerCase()
    .split(/\s+/)
    .map((w) => w.replace(/[^\p{L}\p{N}]/gu, ""))
    .filter(Boolean);

/** Sentences of a finished utterance, so they replace locked rows one for one. */
export function sentences(text: string): string[] {
  const out = text.match(/[^.!?]+[.!?]+["')\]]*|[^.!?]+$/g) ?? [];
  return out.map((s) => s.trim()).filter(Boolean);
}

export class Rows {
  rows: Row[] = [];
  private next = 0;

  private add(utt: number, speaker: string, text: string, at: number) {
    this.rows.push({ uid: this.next++, utt, speaker, text, at });
  }

  /** A caption arrived. Locked rows are kept; a final caption replaces its utterance's rows. */
  caption(c: Caption, now: number): void {
    this.rows = this.rows.filter((r) => now - r.at < ROW_TTL_MS);
    const utt = c.utt ?? -1;
    if (c.final) {
      this.rows = this.rows.filter((r) => !(r.utt === utt && r.speaker === c.speaker));
      for (const s of sentences(c.text)) this.add(utt, c.speaker, s, now);
      return;
    }
    for (const s of c.sealed ?? []) this.add(utt, c.speaker, s, now);
  }

  /** A committed line arrived: drop locked rows it now covers (most of their words appear in it). */
  line(speaker: string, text: string): void {
    const have = new Set(normWords(text));
    this.rows = this.rows.filter((r) => {
      if (isMe(r.speaker) !== isMe(speaker)) return true;
      const w = normWords(r.text);
      if (w.length === 0) return false;
      const hit = w.filter((x) => have.has(x)).length;
      return hit / w.length < 0.6;
    });
  }

  clear(): void {
    this.rows = [];
  }
}

export type GateWord = { word: string; since: number };

/**
 * Which words of the live row are visible. Settled words (index < stable) always are. A
 * tentative word shows once it has stood unchanged for `gateMs`; a word that flips restarts
 * its wait, so words that change inside the gate never appear. Visible words are contiguous.
 */
export function gate(
  prev: GateWord[],
  words: string[],
  stable: number,
  now: number,
  gateMs: number,
): { next: GateWord[]; visible: number } {
  const next: GateWord[] = words.map((w, i) => (prev[i] && prev[i].word === w ? prev[i] : { word: w, since: now }));
  let visible = 0;
  while (visible < words.length && (visible < stable || now - next[visible].since >= gateMs)) visible++;
  return { next, visible };
}

/** Characters per millisecond: a steady typing pace that speeds up when the text is far behind. */
export const TYPE_MS_PER_CHAR = 14;
const CATCH_UP_MS = 350;

export function typed(shown: number, target: number, dtMs: number): number {
  if (shown >= target) return target;
  const backlog = target - shown;
  const rate = Math.max(1 / TYPE_MS_PER_CHAR, backlog / CATCH_UP_MS);
  return Math.min(target, shown + rate * dtMs);
}

/** Length of the shared start of two strings: typed text beyond it must be typed again. */
export function commonPrefix(a: string, b: string): number {
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  return i;
}

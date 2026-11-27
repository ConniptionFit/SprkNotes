// Marks and notes during a session (SPEC 5.1). The backend writes them; this file holds the
// small rules the UI shares with it.
import type { Line } from "./types";

type KeyLike = Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "repeat">;

/** Longest note kept, in characters. Matches `NOTE_MAX_CHARS` in sprk-transcript. */
export const NOTE_MAX_CHARS = 500;

/** Ctrl+M drops a mark while the window has focus. By position, so the keyboard layout does not matter. */
export function isMarkKey(e: KeyLike): boolean {
  return e.code === "KeyM" && e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey && !e.repeat;
}

// Naming a mark, and marking a line that was said earlier. The transcript is only ever added to
// (lines are never rewritten), so both are saved as a note that names the time it belongs to:
// "Mark 00:12:41: Driver swap". The Library shows it as that note; the live pane shows the name on
// the mark, or a mark on the line said at that time, instead of a separate note.


/** Words of a line used as the name when the line itself is marked. */
const LINE_MARK_WORDS = 8;

const LABEL = /^Mark (\d{2,}:\d{2}:\d{2}): ([\s\S]+)$/;

/** The note that names the mark (or marks the line) at time `t`. */
export function markLabel(t: string, name: string): string {
  return `Mark ${t}: ${name.trim().replace(/\s+/g, " ")}`.slice(0, NOTE_MAX_CHARS);
}

/** A note that names a mark: its time and the name; anything else is null. */
export function parseMarkLabel(text: string): { t: string; name: string } | null {
  const m = LABEL.exec(text.trim());
  return m && m[2].trim() ? { t: m[1], name: m[2].trim() } : null;
}

/** The name a marked line gets: its first few words. */
export function lineMarkName(speaker: string, text: string): string {
  const words = text.trim().split(/\s+/).filter(Boolean);
  const head = words.slice(0, LINE_MARK_WORDS).join(" ");
  return `${speaker}: ${head}${words.length > LINE_MARK_WORDS ? "…" : ""}`;
}

export type MarkItem = {
  /** Id of the line the mark sits on: a Mark line, or a speech line that was marked. */
  id: number;
  t: string;
  /** Empty until the person names it. */
  name: string;
};

export type Marks = {
  /** Every mark in time order. */
  items: MarkItem[];
  /** Line id to its mark, for drawing the name on a Mark line or a marked speech line. */
  byLine: Map<number, MarkItem>;
  /** Naming notes that are shown on their mark instead of as a note. */
  hidden: Set<number>;
};

/** Marks in a transcript, with the names their notes give them (the newest name wins). */
export function collectMarks(lines: readonly Line[]): Marks {
  const names = new Map<string, { name: string; id: number }>();
  for (const l of lines) {
    if (l.kind !== "note") continue;
    const p = parseMarkLabel(l.text);
    if (p) names.set(p.t, { name: p.name, id: l.id });
  }
  const items: MarkItem[] = [];
  const byLine = new Map<number, MarkItem>();
  const hidden = new Set<number>();
  const named = new Set<string>();
  const used = new Set<string>();
  for (const l of lines) {
    if (l.kind !== "mark") continue;
    const n = names.get(l.t);
    const item = { id: l.id, t: l.t, name: n?.name ?? "" };
    items.push(item);
    byLine.set(l.id, item);
    if (n) named.add(l.t);
  }
  // A name with no Mark line at its time marks the speech line said then.
  for (const [t, n] of names) {
    if (named.has(t)) {
      hidden.add(n.id);
      continue;
    }
    const line = lines.find((l) => l.kind === "statement" && l.t === t);
    if (!line || used.has(t)) continue;
    used.add(t);
    const item = { id: line.id, t, name: n.name };
    items.push(item);
    byLine.set(line.id, item);
    hidden.add(n.id);
  }
  // Earlier naming notes for the same time are hidden too.
  for (const l of lines) {
    if (l.kind === "note" && !hidden.has(l.id)) {
      const p = parseMarkLabel(l.text);
      if (p && (named.has(p.t) || used.has(p.t))) hidden.add(l.id);
    }
  }
  items.sort((a, b) => (a.t < b.t ? -1 : a.t > b.t ? 1 : a.id - b.id));
  return { items, byLine, hidden };
}

// A note about one line ("Add a note about this line") is saved as "About 00:12:41: text". It is
// an annotation: the live pane and the Library show it nested under the line said at that time,
// flagged as an annotation, instead of as its own entry at the time it was typed.

const ABOUT = /^About (\d{2,}:\d{2}:\d{2}): ([\s\S]+)$/;

/** The start of a note about the line said at time `t`. */
export function aboutPrefix(t: string): string {
  return `About ${t}: `;
}

/** A note about a line: the line's time and the note's own words; anything else is null. */
export function parseAbout(text: string): { t: string; text: string } | null {
  const m = ABOUT.exec(text.trim());
  return m && m[2].trim() ? { t: m[1], text: m[2].trim() } : null;
}

export type LineNote = {
  /** Id of the note line itself. */
  id: number;
  /** When the note was typed. */
  t: string;
  text: string;
};

export type LineNotes = {
  /** Statement id to the notes about it, in the order they were typed. */
  under: Map<number, LineNote[]>;
  /** Notes shown under their line instead of as an entry of their own. */
  hidden: Set<number>;
};

/** Notes about a line, attached to the line said at the time they name. */
export function collectLineNotes(lines: readonly Line[]): LineNotes {
  const under = new Map<number, LineNote[]>();
  const hidden = new Set<number>();
  const firstAt = new Map<string, number>();
  for (const l of lines) if (l.kind === "statement" && !firstAt.has(l.t)) firstAt.set(l.t, l.id);
  for (const l of lines) {
    if (l.kind !== "note") continue;
    const p = parseAbout(l.text);
    const target = p ? firstAt.get(p.t) : undefined;
    if (!p || target === undefined) continue;
    const list = under.get(target) ?? [];
    list.push({ id: l.id, t: l.t, text: p.text });
    under.set(target, list);
    hidden.add(l.id);
  }
  return { under, hidden };
}

/**
 * The line a mark is about: the marked line itself, or for a mark dropped with Mark or Ctrl+M,
 * the line that was being said at that moment (the latest one that started at or before it).
 */
export function markedLine(lines: readonly Line[], m: MarkItem): (Line & { kind: "statement" }) | null {
  let best: (Line & { kind: "statement" }) | null = null;
  for (const l of lines) {
    if (l.kind !== "statement") continue;
    if (l.id === m.id) return l;
    if (l.t <= m.t && (!best || l.t >= best.t)) best = l;
  }
  return best;
}

/** True when a line mark still has the name it got when the line was marked. */
export function isLineMarkDefault(m: MarkItem, line: { speaker: string; text: string } | null): boolean {
  return !!line && m.name === lineMarkName(line.speaker, line.text);
}

/** Lines of a saved transcript, given ids by position, for the helpers above. */
export function withIds(lines: readonly { kind: Line["kind"]; t: string; speaker: string; text: string }[]): Line[] {
  return lines.map((l, id): Line => {
    if (l.kind === "statement") return { kind: "statement", id, t: l.t, speaker: l.speaker, text: l.text };
    if (l.kind === "mark") return { kind: "mark", id, t: l.t };
    return { kind: l.kind, id, t: l.t, text: l.text };
  });
}

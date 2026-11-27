// Find in the live transcript (Ctrl+F). Matching ignores case and runs of spaces; nothing is
// stored or logged, and the search only looks at what the window already shows.

type KeyLike = Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

/** Ctrl+F opens Find. By position, so the keyboard layout does not matter. */
export function isFindKey(e: KeyLike): boolean {
  return e.code === "KeyF" && e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
}

const norm = (q: string) => q.trim().replace(/\s+/g, " ").toLowerCase();

/** Keys of the entries whose text holds the query, in order. An empty query finds nothing. */
export function findKeys(entries: readonly { key: string; text: string }[], query: string): string[] {
  const q = norm(query);
  if (!q) return [];
  return entries.filter((e) => e.text.replace(/\s+/g, " ").toLowerCase().includes(q)).map((e) => e.key);
}

export type Part = { text: string; hit: boolean };

/** Text cut into plain and matching parts, for highlighting. */
export function splitHits(text: string, query: string): Part[] {
  const q = norm(query);
  if (!q) return [{ text, hit: false }];
  const escaped = q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/ /g, "\\s+");
  const re = new RegExp(escaped, "gi");
  const out: Part[] = [];
  let at = 0;
  for (const m of text.matchAll(re)) {
    const i = m.index ?? 0;
    if (i > at) out.push({ text: text.slice(at, i), hit: false });
    out.push({ text: m[0], hit: true });
    at = i + m[0].length;
  }
  if (at < text.length || out.length === 0) out.push({ text: text.slice(at), hit: false });
  return out;
}

/** The next match after `current` (or before it, going back), wrapping around. */
export function step(count: number, current: number, back = false): number {
  if (count <= 0) return -1;
  if (current < 0) return back ? count - 1 : 0;
  return (current + (back ? count - 1 : 1)) % count;
}

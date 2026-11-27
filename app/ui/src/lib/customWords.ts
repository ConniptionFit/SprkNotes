import type { CustomWords } from "./types";

/** The text box as a list: one entry per line, blank lines dropped. The app cleans it further. */
export function lines(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l !== "");
}

/** Has the box changed from what is saved (ignoring blank lines and spacing at the ends)? */
export function changed(text: string, saved: string[]): boolean {
  return JSON.stringify(lines(text)) !== JSON.stringify(saved);
}

/** What the last session did with the list, in plain words. Empty when there is nothing to say. */
export function lastNote(w: CustomWords | null): string {
  const last = w?.last;
  if (!last || last.words === 0) return "";
  const n = last.words === 1 ? "1 custom word" : `${last.words} custom words`;
  return last.hints
    ? `The last session used ${n} as hints for the speech model and fixed near misses.`
    : `The last session used ${n} to fix near misses. This speech model can't take word hints, so the spelling is fixed after it hears the words.`;
}

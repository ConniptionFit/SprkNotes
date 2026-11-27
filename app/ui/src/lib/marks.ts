// Marks and notes during a session (SPEC 5.1). The backend writes them; this file holds the
// small rules the UI shares with it.

type KeyLike = Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "repeat">;

/** Longest note kept, in characters. Matches `NOTE_MAX_CHARS` in sprk-transcript. */
export const NOTE_MAX_CHARS = 500;

/** Ctrl+M drops a mark while the window has focus. By position, so the keyboard layout does not matter. */
export function isMarkKey(e: KeyLike): boolean {
  return e.code === "KeyM" && e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey && !e.repeat;
}

// The background shortcut (SPEC 8.6): reading a key press as a combination, and the words for
// what went wrong. The shell checks the combination again before it registers it.

type KeyLike = Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

/** The key's name from its physical position, so the layout and Shift do not change it. */
function keyName(e: KeyLike): string | null {
  const c = e.code;
  if (/^Key[A-Z]$/.test(c)) return c.slice(3);
  if (/^Digit[0-9]$/.test(c)) return c.slice(5);
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(c)) return c;
  if (c === "Space") return "Space";
  return null;
}

/**
 * The combination a key press makes ("Ctrl+Alt+S"), or null while only modifiers are held or
 * the press could not be a shortcut (no Ctrl, Alt or Win, or a key SprkNotes does not take).
 */
export function comboFromEvent(e: KeyLike): string | null {
  const key = keyName(e);
  if (!key || !(e.ctrlKey || e.altKey || e.metaKey)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Win");
  parts.push(key);
  return parts.join("+");
}

/** A short line for Settings when the shortcut could not be set. */
export function hotkeyProblem(err: unknown): string {
  switch (String(err)) {
    case "taken":
      return "Another app already uses these keys. Pick others.";
    case "invalid":
      return "Use Ctrl, Alt or Win with a letter, a number, a function key or Space.";
    case "unsupported":
      return "The shortcut works on Windows only.";
    default:
      return "Windows did not accept these keys. Try others.";
  }
}

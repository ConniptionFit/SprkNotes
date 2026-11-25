// Window preferences an administrator manages (SPEC 10.2b). The app hands over raw text from the
// policy (locked) and starting values (defaults); this turns it into preference values and drops
// anything that does not fit, so a typo in a deployment script can never break the window.
import type { ManagedPrefs } from "./types";

/** Hover text on every control an administrator locked. */
export const MANAGED_TEXT = "This option is managed by your administrator.";

type Parse = (raw: string) => unknown;
const bool: Parse = (v) => {
  const t = v.trim().toLowerCase();
  if (["1", "true", "on", "yes"].includes(t)) return true;
  if (["0", "false", "off", "no"].includes(t)) return false;
  return undefined;
};
const oneOf = (...allowed: string[]): Parse => (v) => {
  const t = v.trim().toLowerCase();
  return allowed.includes(t) ? t : undefined;
};
const keys: Parse = (v) => {
  const t = v.trim();
  return /^((Ctrl|Alt|Shift|Win)\+)+[A-Za-z0-9]+$/.test(t) && /(Ctrl|Alt|Win)\+/.test(t) ? t : undefined;
};

/** Policy key, the window preference it sets, and how its text is read. */
export const MANAGED_KEYS = {
  Theme: ["theme", oneOf("system", "light", "dark")],
  ThrowbackMode: ["throwback", bool],
  LiveCaptions: ["captionsOn", bool],
  LiveLine: ["liveLine", bool],
  CaptionDelay: ["captionDelay", oneOf("fast", "steady", "smooth")],
  CaptionSpeakers: ["captionsWho", oneOf("others", "both")],
  MicPrompt: ["micPromptOn", bool],
  Hotkey: ["hotkeyOn", bool],
  HotkeyKeys: ["hotkeyKeys", keys],
  AssistantEnabled: ["assistantOn", bool],
} as const satisfies { [key: string]: readonly [string, Parse] };

/** Preference name to value. */
type Prefs = { [key: string]: unknown };

export type Managed = {
  /** Preference name to value. Locked ones win over the person's choice and can't be changed. */
  locked: Prefs;
  /** Used until the person changes that preference themselves. */
  defaults: Prefs;
  /** `SummarizeEnabled=0`: Summarize is removed. */
  summarizeOff: boolean;
};

export const NOTHING_MANAGED: Managed = { locked: {}, defaults: {}, summarizeOff: false };

function pick(raw: { [key: string]: string } | undefined): Prefs {
  const out: Prefs = {};
  for (const [k, v] of Object.entries(raw ?? {})) {
    const entry = Object.entries(MANAGED_KEYS).find(([name]) => name.toLowerCase() === k.toLowerCase());
    if (!entry || typeof v !== "string") continue;
    const [pref, parse] = entry[1];
    const value = parse(v);
    if (value !== undefined) out[pref] = value;
  }
  return out;
}

export function parseManaged(raw: ManagedPrefs | null | undefined): Managed {
  if (!raw) return NOTHING_MANAGED;
  const summarize = Object.entries(raw.locked ?? {}).find(([k]) => k.toLowerCase() === "summarizeenabled");
  return {
    locked: pick(raw.locked),
    defaults: pick(raw.defaults),
    summarizeOff: summarize !== undefined && bool(summarize[1]) === false,
  };
}

/**
 * The preferences in force: built-in defaults, then the administrator's starting values for
 * anything the person has not chosen, then what the person chose, then locked values.
 */
export function effective<T extends Prefs>(
  builtIn: T,
  stored: Prefs,
  chosen: ReadonlySet<string>,
  m: Managed,
): T {
  const out: Prefs = { ...builtIn };
  for (const [k, v] of Object.entries(m.defaults)) if (k in builtIn) out[k] = v;
  for (const [k, v] of Object.entries(stored)) if (k in builtIn && chosen.has(k)) out[k] = v;
  for (const [k, v] of Object.entries(m.locked)) if (k in builtIn) out[k] = v;
  return out as T;
}

/**
 * Which stored preferences the person chose. Stores from before alpha.10 have no list, so a value
 * counts as chosen only when it differs from the built-in default; the rest take an
 * administrator's starting value.
 */
export function chosenKeys(stored: Prefs, builtIn: Prefs): Set<string> {
  if (Array.isArray(stored.chosen)) return new Set(stored.chosen.filter((k): k is string => typeof k === "string"));
  return new Set(
    Object.keys(stored).filter((k) => k in builtIn && JSON.stringify(stored[k]) !== JSON.stringify(builtIn[k])),
  );
}

import { backend } from "./api";

// Small UI-only preferences, shared by the main window and the overlay through localStorage.
// Nothing here is sensitive: a theme choice, caption switches, device names, which sources are on and
// the names of apps the user said not to be asked about.
export type Theme = "system" | "light" | "dark";
export type CaptionWho = "others" | "both";
/** How long a new word waits before it is shown, to let the engine settle it (ADR-008). */
export type CaptionDelay = "fast" | "steady" | "smooth";
export const DELAY_MS: { [k in CaptionDelay]: number } = { fast: 200, steady: 400, smooth: 600 };
type Prefs = {
  theme: Theme;
  /** Throwback Mode: the first look (old icon, Segoe UI, earlier purple) instead of the current brand. */
  throwback: boolean;
  captionsOn: boolean;
  captionsWho: CaptionWho;
  captionDelay: CaptionDelay;
  /** Caption drawer size in logical pixels, and the text size as a step in CAPTION_TEXT. */
  captionW: number;
  captionH: number;
  captionText: number;
  /** Show the live line inside the main transcript, and the transcript text size (step in LINE_TEXT). */
  liveLine: boolean;
  lineText: number;
  /** Ask Sprk (ADR-009). Off by default; the four core features start on, every extra starts off. */
  assistantOn: boolean;
  assistantTier: "titles" | "light" | "standard";
  assistantFeatures: string[];
  /** Microphone picked in Settings; null follows the system default. */
  micDevice: string | null;
  /** Output (the sound that is looped back as Others) picked in Settings; null follows the default. */
  systemDevice: string | null;
  /** Whether Me and Others are transcribed. Remembered across launches; both on at first run. */
  micOn: boolean;
  systemOn: boolean;
  /** Offer to start when another app uses the microphone (SPEC 8.3a), and the apps not to ask about. */
  micPromptOn: boolean;
  micPromptSkip: SkippedApp[];
  /** A keyboard shortcut that starts, pauses and resumes from anywhere (SPEC 8.6). Off by default. */
  hotkeyOn: boolean;
  hotkeyKeys: string;
};
export type SkippedApp = { id: string; name: string };

/** Caption text sizes in px for the magnifier buttons, and the step used by default. */
export const CAPTION_TEXT = [13, 15, 17, 19, 22, 26, 30, 36];
/** Transcript text sizes in px for the main window magnifier buttons. */
export const LINE_TEXT = [12, 13, 14, 15, 17, 19, 22, 26];
export const CAPTION_LIMITS = { minW: 260, maxW: 900, minH: 64, maxH: 480 };

const KEY = "sprknotes.prefs";
const defaults: Prefs = { theme: "system", throwback: false, captionsOn: false, captionsWho: "others", captionDelay: "steady", captionW: 420, captionH: 118, captionText: 2, liveLine: true, lineText: 3, assistantOn: false, assistantTier: "light", assistantFeatures: ["title", "recap", "actions", "ask"], micDevice: null, systemDevice: null, micOn: true, systemOn: true, micPromptOn: true, micPromptSkip: [], hotkeyOn: false, hotkeyKeys: "Ctrl+Alt+S" };

const clamp = (n: unknown, lo: number, hi: number, fallback: number) =>
  typeof n === "number" && Number.isFinite(n) ? Math.min(hi, Math.max(lo, Math.round(n))) : fallback;

function load(): Prefs {
  try {
    const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(KEY);
    const p: Prefs = { ...defaults, ...(raw ? JSON.parse(raw) : {}) };
    // A hand-edited or older value must never break the overlay.
    p.captionW = clamp(p.captionW, CAPTION_LIMITS.minW, CAPTION_LIMITS.maxW, defaults.captionW);
    p.captionH = clamp(p.captionH, CAPTION_LIMITS.minH, CAPTION_LIMITS.maxH, defaults.captionH);
    p.captionText = clamp(p.captionText, 0, CAPTION_TEXT.length - 1, defaults.captionText);
    if (!(p.captionDelay in DELAY_MS)) p.captionDelay = defaults.captionDelay;
    p.lineText = clamp(p.lineText, 0, LINE_TEXT.length - 1, defaults.lineText);
    if (!["titles", "light", "standard"].includes(p.assistantTier)) p.assistantTier = defaults.assistantTier;
    if (!Array.isArray(p.assistantFeatures)) p.assistantFeatures = defaults.assistantFeatures;
    if (typeof p.micDevice !== "string" || !p.micDevice) p.micDevice = null;
    if (typeof p.systemDevice !== "string" || !p.systemDevice) p.systemDevice = null;
    if (typeof p.micOn !== "boolean") p.micOn = true;
    if (typeof p.systemOn !== "boolean") p.systemOn = true;
    if (typeof p.micPromptOn !== "boolean") p.micPromptOn = true;
    p.micPromptSkip = Array.isArray(p.micPromptSkip)
      ? p.micPromptSkip.filter((a) => a && typeof a.id === "string" && a.id && typeof a.name === "string")
      : [];
    if (typeof p.hotkeyOn !== "boolean") p.hotkeyOn = false;
    if (typeof p.hotkeyKeys !== "string" || !p.hotkeyKeys) p.hotkeyKeys = defaults.hotkeyKeys;
    return p;
  } catch {
    return { ...defaults };
  }
}

export const prefs = $state<Prefs>(load());

export function setPref<K extends keyof Prefs>(key: K, value: Prefs[K]) {
  prefs[key] = value;
  try {
    localStorage.setItem(KEY, JSON.stringify(prefs));
  } catch {
    /* storage can be blocked; the choice then lasts until the window closes */
  }
}

if (typeof window !== "undefined") {
  // The other window changed a preference.
  window.addEventListener("storage", (e) => {
    if (e.key === KEY) Object.assign(prefs, load());
  });
}

/** Keep <html data-theme> in step. The overlay is dark unless the app theme is explicitly Light. */
export function bindTheme(overlay: boolean) {
  $effect.root(() => {
    $effect(() => {
      const t: Theme = overlay ? (prefs.theme === "light" ? "light" : "dark") : prefs.theme;
      const el = document.documentElement;
      if (t === "system") el.removeAttribute("data-theme");
      else el.setAttribute("data-theme", t);
      if (prefs.throwback) el.setAttribute("data-brand", "throwback");
      else el.removeAttribute("data-brand");
      // The tray and taskbar icons live in the shell; only the main window tells it.
      if (!overlay) backend.then((b) => b.setBrand(prefs.throwback)).catch(() => {});
    });
  });
}

/** What the engine should produce: (captions on, include Me). Both windows send the same answer. */
export function captionFlags(): [boolean, boolean] {
  // The main window's live line shows both speakers, so it wants Me's words as they are spoken.
  // Without them Me only appeared once the line was written, six seconds or more later.
  return [prefs.captionsOn || prefs.liveLine, prefs.captionsWho === "both" || prefs.liveLine];
}

// Small UI-only preferences, shared by the main window and the overlay through localStorage.
// Nothing here is sensitive: a theme choice and two caption switches.
export type Theme = "system" | "light" | "dark";
export type CaptionWho = "others" | "both";
type Prefs = {
  theme: Theme;
  captionsOn: boolean;
  captionsWho: CaptionWho;
  /** Caption drawer size in logical pixels, and the text size as a step in CAPTION_TEXT. */
  captionW: number;
  captionH: number;
  captionText: number;
  /** Show the live line inside the main transcript, and the transcript text size (step in LINE_TEXT). */
  liveLine: boolean;
  lineText: number;
};

/** Caption text sizes in px for the magnifier buttons, and the step used by default. */
export const CAPTION_TEXT = [13, 15, 17, 19, 22, 26, 30, 36];
/** Transcript text sizes in px for the main window magnifier buttons. */
export const LINE_TEXT = [12, 13, 14, 15, 17, 19, 22, 26];
export const CAPTION_LIMITS = { minW: 260, maxW: 900, minH: 64, maxH: 480 };

const KEY = "sprknotes.prefs";
const defaults: Prefs = { theme: "system", captionsOn: false, captionsWho: "others", captionW: 420, captionH: 118, captionText: 2, liveLine: true, lineText: 3 };

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
    p.lineText = clamp(p.lineText, 0, LINE_TEXT.length - 1, defaults.lineText);
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
    });
  });
}

/** What the engine should produce: (captions on, include Me). Both windows send the same answer. */
export function captionFlags(): [boolean, boolean] {
  return [prefs.captionsOn || prefs.liveLine, prefs.captionsWho === "both"];
}

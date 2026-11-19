// Small UI-only preferences, shared by the main window and the overlay through localStorage.
// Nothing here is sensitive: a theme choice and two caption switches.
export type Theme = "system" | "light" | "dark";
export type CaptionWho = "others" | "both";
type Prefs = { theme: Theme; captionsOn: boolean; captionsWho: CaptionWho };

const KEY = "sprknotes.prefs";
const defaults: Prefs = { theme: "system", captionsOn: false, captionsWho: "others" };

function load(): Prefs {
  try {
    const raw = typeof localStorage === "undefined" ? null : localStorage.getItem(KEY);
    return { ...defaults, ...(raw ? JSON.parse(raw) : {}) };
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

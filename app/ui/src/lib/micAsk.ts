// Wording and the skip list for "Start a transcript?" (SPEC 8.3a).
import type { SkippedApp } from "./prefs.svelte";

export type MicApp = { id: string; name: string };
/** What the shell sends the overlay: whether the question is up, and which apps it is about. */
export type MicAsk = { up: boolean; apps: MicApp[] };
export const NO_ASK: MicAsk = { up: false, apps: [] };

/** "Zoom", "Zoom and Teams", "Zoom, Teams and Discord". */
export function appList(apps: MicApp[]): string {
  const names = apps.map((a) => a.name);
  if (names.length <= 1) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** The question. Without a name (an older shell) it reads as before. */
export function askText(apps: MicApp[]): string {
  if (apps.length === 0) return "Your microphone is in use. Start a transcript?";
  return `${appList(apps)} ${apps.length === 1 ? "is" : "are"} using your microphone. Start a transcript?`;
}

/** The third button: names one app, or says "these apps". */
export function skipLabel(apps: MicApp[]): string {
  return apps.length === 1 ? `Don't ask for ${apps[0].name}` : "Don't ask for these apps";
}

/** The skip list with these apps added once each (ids compared without case). */
export function withSkipped(list: SkippedApp[], apps: MicApp[]): SkippedApp[] {
  const out = [...list];
  for (const a of apps) {
    if (!out.some((s) => s.id.toLowerCase() === a.id.toLowerCase())) out.push({ id: a.id, name: a.name });
  }
  return out;
}

/** Older shells send a bare boolean; newer ones the full question. */
export function asAsk(v: unknown): MicAsk {
  if (typeof v === "boolean") return { up: v, apps: [] };
  if (v && typeof v === "object" && "up" in v) {
    const o = v as { up: unknown; apps?: unknown };
    const apps = Array.isArray(o.apps)
      ? o.apps.filter((a): a is MicApp => !!a && typeof a.id === "string" && typeof a.name === "string")
      : [];
    return { up: !!o.up, apps };
  }
  return NO_ASK;
}

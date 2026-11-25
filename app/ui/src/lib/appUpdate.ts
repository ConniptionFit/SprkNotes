import type { AppUpdateProgress } from "./types";

/** What Settings shows while "Update now" runs. */
export function progressText(p: AppUpdateProgress | null): string {
  if (!p) return "";
  if (p.stage === "install") return "Installing. SprkNotes closes now and opens again when the update is done.";
  return p.percent == null ? "Downloading the update…" : `Downloading the update… ${p.percent}%`;
}

/** Bar width in percent: unknown while downloading without a size, full while installing. */
export function progressValue(p: AppUpdateProgress | null): number | null {
  if (!p) return null;
  if (p.stage === "install") return 100;
  return p.percent == null ? null : Math.max(0, Math.min(100, p.percent));
}

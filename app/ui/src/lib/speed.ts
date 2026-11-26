import type { SpeedStatus } from "./types";

const threadWord = (n: number) => (n === 1 ? "1 thread" : `${n} threads`);

/** The read-only line under the transcription model in Settings (SPEC 16.8a). Empty: show nothing. */
export function speedText(s: SpeedStatus | null): string {
  if (!s) return "";
  switch (s.state) {
    case "done":
      return s.speed !== null && s.threads !== null
        ? `Speed: ${s.speed.toFixed(1)}x real time on ${threadWord(s.threads)}`
        : "Speed: not checked yet";
    case "checking":
      return "Checking the speed on this PC…";
    case "env":
      return s.threads !== null ? `Speed: set to ${threadWord(s.threads)} on this PC` : "";
    case "none":
      return "Speed: not checked yet";
    default:
      return "";
  }
}

/** The button can run a check only when there is a model, no check is running and the setting is not fixed. */
export function canCheckSpeed(s: SpeedStatus | null): boolean {
  return !!s && (s.state === "done" || s.state === "none");
}

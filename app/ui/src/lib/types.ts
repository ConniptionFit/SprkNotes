export type Status = "ready" | "transcribing" | "paused";

/** One line of the live pane, matching the transcript grammar (SPEC 5.1). */
export type Line =
  | { kind: "statement"; id: number; t: string; speaker: string; text: string }
  | { kind: "event"; id: number; t: string; text: string };

export type Source = "mic" | "system";

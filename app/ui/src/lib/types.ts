export type Status = "ready" | "transcribing" | "paused";

/** One line of the live pane, matching the transcript grammar (SPEC 5.1). */
export type Line =
  | { kind: "statement"; id: number; t: string; speaker: string; text: string }
  | { kind: "event"; id: number; t: string; text: string };

export type Source = "mic" | "system";

/** Mirrors the Rust `ModelStatus` (camelCase). No network call behind it. */
export type ModelStatus = {
  ready: boolean;
  downloadEnabled: boolean;
  updateCheckEnabled: boolean;
  customSource: boolean;
  busy: boolean;
};

export type PackInfo = { id: string; version: string; installed?: string | null };
export type UpdateInfo = { available: boolean; packs: PackInfo[] };

export type DownloadStage = "checking" | "downloading" | "verifying" | "done" | "error";
export type DownloadProgress = {
  stage: DownloadStage;
  pack?: string | null;
  done: number;
  total?: number | null;
  /** Already plain language, never a URL. Shown as is. */
  error?: string | null;
};

/** A live caption: a partial (final=false) that later settles into a committed transcript line. */
export type Caption = { speaker: string; text: string; final: boolean };

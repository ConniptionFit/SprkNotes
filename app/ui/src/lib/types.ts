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
export type Caption = {
  speaker: string;
  /** The whole line so far. Earlier words can change between updates (corrections). */
  text: string;
  /** How many leading words are settled; the rest are tentative. */
  stable: number;
  /** The utterance finished: text is the committed wording. */
  final: boolean;
  /** Rows that locked and left the live row since the last caption (ADR-008). Oldest first. */
  sealed?: string[];
  /** Which utterance this belongs to, so the final one replaces its locked rows. */
  utt?: number;
};

/** One transcript in the library list. */
export type LibraryEntry = {
  path: string;
  name: string;
  title: string;
  date: string;
  start: string;
  duration: string;
  status: string;
  folder: string;
};

export type ViewLine = { kind: "statement" | "event"; t: string; speaker: string; text: string };
export type TranscriptView = {
  title: string;
  date: string;
  start: string;
  duration: string;
  context: string;
  lines: ViewLine[];
};

export type HandoffTarget = { id: string; name: string };
/** Result of Summarize: either a choice to make, or what happened. */
export type Summarized = { choose: HandoffTarget[]; toasts: string[]; clipboard?: string | null };
export type ExportFormat = "txt" | "json" | "docx";

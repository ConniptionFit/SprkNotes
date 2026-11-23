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

/** Mirrors the Rust `UpdateStatus` (camelCase): app updates, and which settings policy locks. */
export type AppUpdateStatus = {
  installed: string;
  mode: "off" | "notify" | "auto" | "force";
  channel: "stable" | "beta" | "alpha";
  repoUrl: string;
  modeLocked: boolean;
  channelLocked: boolean;
  repoLocked: boolean;
  busy: boolean;
  readyToInstall: boolean;
  /** Per-machine install: a background task keeps it current. */
  managedInstall: boolean;
};
export type AppUpdateInfo = { available: boolean; version?: string | null; notesUrl?: string | null };

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

/** Ask Sprk (ADR-009). Mirrors `assistant.rs`; every error is a short code, never text from the meeting. */
export type AssistantFeature =
  | "title" | "recap" | "actions" | "ask"
  | "agenda" | "commitments" | "pins" | "parkinglot" | "explain" | "shorten" | "prep"
  | "asked" | "summary" | "rolling";
export type AssistantTier = "titles" | "light" | "standard";
export type AssistantStatus = {
  state: "off" | "noModel" | "notEnoughMemory" | "ready";
  /** Policy decides on/off, so the switch is read-only. */
  locked: boolean;
  canDownload: boolean;
  model: string | null;
  tier: string;
  maxTier: string;
  enabled: boolean;
  features: AssistantFeature[];
  allowedFeatures: AssistantFeature[];
};
export type AssistantRequest =
  | { op: "title" }
  | { op: "recap"; minutes: number }
  | { op: "actions" }
  | { op: "ask"; question: string }
  | { op: "agenda"; items: string[] }
  | { op: "commitments" }
  | { op: "explain"; lineId: number }
  | { op: "shorten"; fromId: number; toId: number }
  | { op: "prep" }
  | { op: "summary" }
  | { op: "asked" }
  | { op: "catchups" }
  | { op: "pinNow"; label: string }
  | { op: "unpin"; lineId: number }
  | { op: "pins" }
  | { op: "parking" };
export type Cited = { text: string; cites: number[] };
export type ActionOut = { task: string; ownerKind: "me" | "named" | "unknown"; ownerName: string | null; due: string | null; cites: number[] };
export type AssistantResponse =
  | { kind: "title"; text: string | null }
  | { kind: "recap"; bullets: Cited[]; dropped: number; nothingSaid: boolean }
  | { kind: "actions"; items: ActionOut[]; dropped: number }
  | { kind: "answer"; state: "ok" | "notStated" | "unconfirmed"; text: string | null; cites: number[] }
  | { kind: "agenda"; items: { item: string; covered: boolean; cites: number[] }[] }
  | { kind: "commitments"; items: ActionOut[] }
  | { kind: "pins"; pins: { lineId: number; t: string; label: string }[] }
  | { kind: "parking"; items: { lineId: number; t: string; text: string }[] }
  | { kind: "prep"; markdown: string }
  | { kind: "asked"; items: { lineId: number; t: string; text: string }[] }
  | { kind: "catchups"; items: { from: string; to: string; bullets: Cited[] }[] };

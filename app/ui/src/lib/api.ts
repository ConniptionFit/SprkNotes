import type { ManagedPrefs, Retention, SaveLocation } from "./types";
import type { AppUpdateInfo, AppUpdateProgress, AppUpdateStatus, AssistantRequest, AssistantModelStatus, AssistantResponse, AssistantStatus, Caption, DownloadProgress, ExportFormat, LibraryEntry, Line, ModelStatus, Phase, Source, Status, Summarized, TranscriptView, UpdateInfo } from "./types";
import { asAsk, NO_ASK, type MicAsk } from "./micAsk";

/** Typed surface between the UI and the Rust side. Every command has a matching Tauri command. */
export interface Backend {
  start(name: string): Promise<void>;
  /** Start a session that adds to an existing transcript (Library "Continue transcribing"). */
  continueTranscript(path: string): Promise<void>;
  /** Where transcripts are saved, and whether an administrator set it. */
  saveLocation(): Promise<SaveLocation>;
  /** Window preferences an admin set: locked ones are read-only, defaults apply until changed. */
  managedPrefs(): Promise<ManagedPrefs>;
  /** Save a new folder (may use %USERPROFILE%-style names); null goes back to the default. */
  setSaveLocation(path: string | null): Promise<SaveLocation>;
  /** Open a library folder ("" is the top) in File Explorer. */
  libraryOpenFolder(folder: string): Promise<void>;
  /** "Keep transcripts for": days (0 is forever) and whether an administrator set it. */
  retention(): Promise<Retention>;
  /** Save "Keep transcripts for" (0, 30, 90 or 365 days). Applies at the next start or daily check. */
  setRetention(days: number): Promise<Retention>;
  /** Old transcripts went to the Recycle Bin (a count only). */
  onRetentionRemoved(cb: (r: { removed: number; days: number }) => void): Promise<() => void>;
  /** The Library closed its transcript view (retention leaves an open one alone). */
  libraryClose(): Promise<void>;
  pause(): Promise<void>;
  resume(): Promise<void>;
  end(): Promise<void>;
  rename(name: string): Promise<void>;
  setSource(source: Source, on: boolean): Promise<void>;
  /** Devices a source can be switched to, and the pick (null: the system default). */
  listDevices(source: Source): Promise<string[]>;
  setDevice(source: Source, name: string | null): Promise<void>;
  modelStatus(): Promise<ModelStatus>;
  modelCheckUpdate(): Promise<UpdateInfo>;
  modelDownload(): Promise<void>;
  /** App updates (ADR-011). Policy-locked settings refuse changes with a short sentence. */
  appUpdateStatus(): Promise<AppUpdateStatus>;
  appUpdateCheck(): Promise<AppUpdateInfo>;
  appUpdateSet(change: { channel?: string; repoUrl?: string; mode?: string }): Promise<void>;
  /** Downloads and verifies; installs when the app closes. Resolves to the new version, or null. */
  appUpdateInstall(): Promise<string | null>;
  /** "Update now": downloads with progress, then closes SprkNotes, installs with Windows Installer's
   * progress window and starts the app again. Resolves to the new version, or null when current.
   * Rejects with a short sentence (for example while a transcript is running). */
  appUpdateNow(): Promise<string | null>;
  onAppUpdateProgress(cb: (p: AppUpdateProgress) => void): Promise<() => void>;
  onModelDownload(cb: (p: DownloadProgress) => void): Promise<() => void>;
  onModelUpdate(cb: (u: UpdateInfo) => void): Promise<() => void>;
  /** Overlay only: size the floating window to its content (logical pixels). */
  miniResize(width: number, height: number, flipped: boolean): Promise<void>;
  /** Overlay only: true when the caption drawer should sit above the pill (pill low on screen). */
  miniFlipped(): Promise<boolean>;
  /** Overlay only: the host moved the pill to the other half of the screen. */
  onMiniFlip(cb: (flipped: boolean) => void): Promise<() => void>;
  /** "Start a transcript?" while another app holds the microphone and the app is in the
   * background (SPEC 8.3a): whether it is up now, changes to it, and the answer. */
  micPromptPending(): Promise<MicAsk>;
  onMicPrompt(cb: (ask: MicAsk) => void): Promise<() => void>;
  micPromptAnswer(start: boolean): Promise<void>;
  /** Turn the question on or off, and set the apps (ids) it should not ask about. */
  setMicPrompt(enabled: boolean, skipped: string[]): Promise<void>;
  /** The background shortcut: on or off, and its keys. Rejects with "invalid", "taken", "failed" or "unsupported". */
  setHotkey(enabled: boolean, keys: string): Promise<string>;
  /** Tell the engine which caption sources are wanted, so unused ones cost nothing. */
  setCaptions(on: boolean, me: boolean): Promise<void>;
  /** Swap the tray and window icons between the current brand and Throwback Mode. */
  setBrand(throwback: boolean): Promise<void>;
  /** Partial caption text while someone is still speaking (never saved). */
  onCaption(cb: (c: Caption) => void): Promise<() => void>;
  /** Input levels for the Me and Others buttons, 0 to 1, about twenty a second while transcribing. */
  onLevels(cb: (levels: [number, number]) => void): Promise<() => void>;
  /** Mini controller only: bring the main window back. */
  openApp(): Promise<void>;
  /** Library: transcripts newest first; the query matches names and full text. */
  libraryList(query: string): Promise<LibraryEntry[]>;
  libraryOpen(path: string): Promise<TranscriptView>;
  /** "View in folder". */
  libraryReveal(path: string): Promise<void>;
  /** Saves next to the transcript and shows it; returns the new file name. */
  libraryExport(path: string, format: ExportFormat): Promise<string>;
  /** Newest transcript, or null when there is none yet. */
  latestTranscript(): Promise<LibraryEntry | null>;
  /**
   * Hand a transcript to the Claude or Copilot desktop app. With no target: one app installed is
   * used, none copies the prompt and shows the file, and both come back as `choose`.
   */
  summarize(path: string | null, target: string | null): Promise<Summarized>;
  /** Library folders as paths relative to the library ("Clients/Acme"), empty ones included. */
  libraryFolders(): Promise<string[]>;
  /** Rename a transcript (title and file name). Returns its new path. */
  libraryRename(path: string, title: string): Promise<string>;
  /** Move a transcript into `folder` ("" is the top of the library). Returns its new path. */
  libraryMove(path: string, folder: string): Promise<string>;
  /** Move a transcript to the Recycle Bin. The window asks first. */
  libraryDelete(path: string): Promise<void>;
  /** New folder inside `parent` ("" for the top); rename a folder. Both return the folder's path. */
  folderCreate(parent: string, name: string): Promise<string>;
  folderRename(folder: string, name: string): Promise<string>;
  /** Transcripts a crash left unfinished (asked once at launch), and closing one as interrupted. */
  recoveryScan(): Promise<LibraryEntry[]>;
  recoveryMark(path: string): Promise<void>;
  /** A session problem such as a line that could not be saved: `write_failed`, `finalize_failed`. */
  onProblem(cb: (kind: string) => void): Promise<() => void>;
  /** Ask Sprk. Off by default; every call can fail with a short code ("off", "noModel", "busy", ...). */
  assistantStatus(): Promise<AssistantStatus>;
  assistantSet(enabled: boolean, tier: string, features: string[]): Promise<void>;
  assistantCancel(): Promise<void>;
  assistantRun(request: AssistantRequest): Promise<AssistantResponse>;
  /** Ask Sprk model packs (ADR-010). Downloads reuse the model index; progress arrives as events. */
  assistantModelStatus(): Promise<AssistantModelStatus>;
  assistantModelCheck(tier: string): Promise<UpdateInfo>;
  assistantModelDownload(tier: string): Promise<void>;
  onAssistantModelDownload(cb: (p: DownloadProgress) => void): Promise<() => void>;
  onAssistantModelUpdate(cb: (u: UpdateInfo) => void): Promise<() => void>;
  onLine(cb: (l: Line) => void): Promise<() => void>;
  /** A line shown live was dropped as microphone bleed; its live row should go. Never saved. */
  onDropped(cb: (d: { speaker: string; text: string }) => void): Promise<() => void>;
  onStatus(cb: (s: Status) => void): Promise<() => void>;
  onPhase(cb: (p: Phase) => void): Promise<() => void>;
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function tauri(): Promise<Backend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  return {
    start: (name) => invoke("start_session", { name }),
    pause: () => invoke("pause_session"),
    resume: () => invoke("resume_session"),
    end: () => invoke("end_session"),
    rename: (name) => invoke("rename_session", { name }),
    setSource: (source, on) => invoke("set_source", { source, on }),
    listDevices: (source) => invoke("list_devices", { source }),
    setDevice: (source, name) => invoke("set_device", { source, name }),
    modelStatus: () => invoke("model_status"),
    modelCheckUpdate: () => invoke("model_check_update"),
    modelDownload: () => invoke("model_download"),
    appUpdateStatus: () => invoke("update_status"),
    appUpdateCheck: () => invoke("update_check"),
    appUpdateSet: ({ channel, repoUrl, mode }) => invoke("update_set", { channel, repoUrl, mode }),
    appUpdateInstall: () => invoke("update_install"),
    appUpdateNow: () => invoke("update_now"),
    onAppUpdateProgress: (cb) => listen<AppUpdateProgress>("app-update-progress", (e) => cb(e.payload)),
    onModelDownload: (cb) => listen<DownloadProgress>("model-download", (e) => cb(e.payload)),
    onModelUpdate: (cb) => listen<UpdateInfo>("model-update", (e) => cb(e.payload)),
    miniResize: (width, height, flipped) => invoke("mini_resize", { width, height, flipped }),
    miniFlipped: () => invoke<boolean>("mini_flipped"),
    onMiniFlip: (cb) => listen<boolean>("mini-flip", (e) => cb(e.payload)),
    micPromptPending: () => invoke<unknown>("mic_prompt_pending").then(asAsk),
    onMicPrompt: (cb) => listen<unknown>("mic-prompt", (e) => cb(asAsk(e.payload))),
    micPromptAnswer: (start) => invoke("mic_prompt_answer", { start }),
    setMicPrompt: (enabled, skipped) => invoke("set_mic_prompt", { enabled, skipped }),
    setHotkey: (enabled, keys) => invoke<string>("set_hotkey", { enabled, keys }),
    setCaptions: (on, me) => invoke("set_captions", { on, me }),
    setBrand: (throwback) => invoke("set_brand", { throwback }),
    onCaption: (cb) => listen<Caption>("caption", (e) => cb(e.payload)),
    onLevels: (cb) => listen<[number, number]>("source-levels", (e) => cb(e.payload)),
    openApp: () => invoke("open_app"),
    libraryList: (query) => invoke("library_list", { query }),
    libraryOpen: (path) => invoke("library_open", { path }),
    libraryClose: () => invoke("library_close"),
    retention: () => invoke("retention_status"),
    setRetention: (days) => invoke("retention_set", { days }),
    onRetentionRemoved: (cb) => listen<{ removed: number; days: number }>("retention-removed", (e) => cb(e.payload)),
    libraryReveal: (path) => invoke("library_reveal", { path }),
    libraryOpenFolder: (folder) => invoke("library_open_folder", { folder }),
    continueTranscript: (path) => invoke("continue_session", { path }),
    saveLocation: () => invoke("save_location_status"),
    managedPrefs: () => invoke("managed_prefs"),
    setSaveLocation: (path) => invoke("save_location_set", { path }),
    libraryExport: (path, format) => invoke("library_export", { path, format }),
    latestTranscript: () => invoke("latest_transcript"),
    summarize: (path, target) => invoke("summarize", { path, target }),
    libraryFolders: () => invoke("library_folders"),
    libraryRename: (path, title) => invoke("library_rename", { path, title }),
    libraryMove: (path, folder) => invoke("library_move", { path, folder }),
    libraryDelete: (path) => invoke("library_delete", { path }),
    folderCreate: (parent, name) => invoke("folder_create", { parent, name }),
    folderRename: (folder, name) => invoke("folder_rename", { folder, name }),
    recoveryScan: () => invoke("recovery_scan"),
    recoveryMark: (path) => invoke("recovery_mark", { path }),
    onProblem: (cb) => listen<string>("session-problem", (e) => cb(e.payload)),
    assistantStatus: () => invoke("assistant_status"),
    assistantSet: (enabled, tier, features) => invoke("assistant_set", { enabled, tier, features }),
    assistantCancel: () => invoke("assistant_cancel"),
    assistantRun: (request) => invoke("assistant_run", { request }),
    assistantModelStatus: () => invoke("assistant_model_status"),
    assistantModelCheck: (tier) => invoke("assistant_model_check", { tier }),
    assistantModelDownload: (tier) => invoke("assistant_model_download", { tier }),
    onAssistantModelDownload: (cb) => listen<DownloadProgress>("assistant-model-download", (e) => cb(e.payload)),
    onAssistantModelUpdate: (cb) => listen<UpdateInfo>("assistant-model-update", (e) => cb(e.payload)),
    onLine: (cb) => listen<Line>("transcript-line", (e) => cb(e.payload)),
    onDropped: (cb) => listen<{ speaker: string; text: string }>("transcript-dropped", (e) => cb(e.payload)),
    onStatus: (cb) => listen<Status>("session-status", (e) => cb(e.payload)),
    onPhase: (cb) => listen<Phase>("session-phase", (e) => cb(e.payload)),
  };
}

const mockFolders: string[] = ["Clients", "Clients/Acme"];
// "?save=managed" previews a folder an administrator set.
// Preview a managed PC with ?managed=1 (captions locked on, dark theme as a starting value).
const mockManaged: ManagedPrefs =
  typeof location !== "undefined" && new URLSearchParams(location.search).get("managed")
    ? { locked: { LiveCaptions: "1", CaptionSpeakers: "both" }, defaults: { Theme: "dark" } }
    : { locked: {}, defaults: {} };

const mockSave: SaveLocation = {
  path: "C:\\Users\\you\\Documents\\SprkNotes\\Transcripts",
  chosen: null,
  source: "default",
  locked: false,
};
if (typeof location !== "undefined" && new URLSearchParams(location.search).get("save") === "managed") {
  Object.assign(mockSave, { path: "C:\\Users\\you\\OneDrive - Contoso\\Transcripts", source: "policy", locked: true });
}

const mockRetention: Retention = { days: 0, source: "default", locked: false };
if (typeof location !== "undefined" && new URLSearchParams(location.search).get("retention") === "managed") {
  Object.assign(mockRetention, { days: 180, source: "policy", locked: true });
}

const mockLibrary: LibraryEntry[] = [
  { path: "/mock/2026-11-19 0900 Budget sync.md", name: "2026-11-19 0900 Budget sync", title: "Budget sync", date: "2026-11-19", start: "09:00", duration: "0:32:10", status: "complete", folder: "" },
  { path: "/mock/2026-11-18 1400 Roadmap review.md", name: "2026-11-18 1400 Roadmap review", title: "Roadmap review", date: "2026-11-18", start: "14:00", duration: "0:48:02", status: "complete", folder: "" },
];

const CORE = ["title", "recap", "actions", "ask"] as const;

/** Synthetic Ask Sprk answers for the browser preview. "?assistant=nomodel|memory" previews the other states. */
function mockAssistant(): Pick<
  Backend,
  | "assistantStatus" | "assistantSet" | "assistantCancel" | "assistantRun"
  | "assistantModelStatus" | "assistantModelCheck" | "assistantModelDownload" | "onAssistantModelDownload" | "onAssistantModelUpdate"
> {
  const flag = typeof location !== "undefined" ? new URLSearchParams(location.search).get("assistant") : null;
  let on = false;
  // "?assistant=nomodel" starts with no model so the download button and the disabled switch show.
  let installed = flag !== "nomodel";
  let dlCb: (p: DownloadProgress) => void = () => {};
  let features: string[] = [...CORE];
  let pins: { lineId: number; t: string; label: string }[] = [];
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  return {
    assistantStatus: async () => ({
      state: !on ? "off" : !installed ? "noModel" : flag === "memory" ? "notEnoughMemory" : "ready",
      locked: false,
      canDownload: true,
      model: on ? "mock-light@2027.01.0" : null,
      tier: "light",
      maxTier: "standard",
      enabled: on,
      features: features as AssistantStatus["features"],
      allowedFeatures: ["title", "recap", "actions", "ask", "agenda", "commitments", "pins", "parkinglot", "explain", "shorten", "prep", "asked", "summary", "rolling"],
    }),
    assistantSet: async (enabled, _tier, f) => {
      on = enabled;
      features = f;
    },
    assistantCancel: async () => {},
    assistantModelStatus: async () => ({
      installed: installed ? [{ id: "mock-light", version: "2027.01.0", tier: "light" }] : [],
      anyInstalled: installed,
      downloadEnabled: true,
      updateCheckEnabled: true,
      customSource: false,
      busy: false,
    }),
    assistantModelCheck: async () => ({ available: false, packs: [] }),
    assistantModelDownload: async () => {
      const total = 1_500_000_000;
      let done = 0;
      dlCb({ stage: "checking", done: 0 });
      const t = setInterval(() => {
        done += 100_000_000;
        if (done < total) dlCb({ stage: "downloading", pack: "Ask Sprk model", done, total });
        else {
          clearInterval(t);
          dlCb({ stage: "verifying", done: total, total });
          setTimeout(() => ((installed = true), dlCb({ stage: "done", done: total, total })), 700);
        }
      }, 250);
    },
    onAssistantModelDownload: async (cb) => ((dlCb = cb), () => {}),
    onAssistantModelUpdate: async () => () => {},
    assistantRun: async (r) => {
      if (!on) throw "off";
      await wait(600);
      switch (r.op) {
        case "title": return { kind: "title", text: "Budget review" };
        case "recap": return { kind: "recap", bullets: [{ text: "They went over the synthetic budget.", cites: [1] }, { text: "A reply came from the other side.", cites: [2] }], dropped: 0, nothingSaid: false };
        case "actions": return { kind: "actions", items: [{ task: "Send the sample sheet", ownerKind: "me", ownerName: null, due: "Thursday", cites: [3] }], dropped: 0 };
        case "ask": return { kind: "answer", state: "ok", text: "The numbers are in the shared sheet.", cites: [2] };
        case "agenda": return { kind: "agenda", items: r.items.map((item, i) => ({ item, covered: i === 0, cites: i === 0 ? [1] : [] })) };
        case "commitments": return { kind: "commitments", items: [{ task: "Send the sample sheet", ownerKind: "me", ownerName: null, due: "Thursday", cites: [3] }] };
        case "explain": return { kind: "answer", state: "ok", text: "They meant the shared budget sheet.", cites: [r.lineId] };
        case "shorten": return { kind: "answer", state: "ok", text: "The budget is in the sheet.", cites: [r.fromId] };
        case "prep": return { kind: "prep", markdown: "# Next meeting\n\n## Open action items\n- Send the sample sheet (Me, Thursday)\n" };
        case "summary": return { kind: "recap", bullets: [{ text: "The meeting covered the synthetic budget.", cites: [1] }], dropped: 0, nothingSaid: false };
        case "asked": return { kind: "asked", items: [{ lineId: 5, t: "00:00:50", text: "Can you send the sheet today?" }] };
        case "catchups": return { kind: "catchups", items: [{ from: "00:00:00", to: "00:10:00", bullets: [{ text: "The first ten minutes covered the budget.", cites: [1] }] }] };
        case "pinNow": pins = [...pins, { lineId: pins.length + 1, t: "00:00:10", label: r.label }]; return { kind: "pins", pins };
        case "unpin": pins = pins.filter((p) => p.lineId !== r.lineId); return { kind: "pins", pins };
        case "pins": return { kind: "pins", pins };
        case "parking": return { kind: "parking", items: [{ lineId: 4, t: "00:00:40", text: "Let's take the hiring plan offline." }] };
      }
    },
  };
}

/** Browser-only stand-in with synthetic text, so the UI can be built and checked without Tauri. */
export function mockBackend(): Backend {
  const upd = { channel: "stable" as "stable" | "beta" | "alpha" };
  let lineCb: (l: Line) => void = () => {};
  let statusCb: (s: Status) => void = () => {};
  let timer: ReturnType<typeof setInterval> | undefined;
  let id = 0;
  let t0 = 0;
  // Preview switches for the browser mock: ?model=missing | managed | update
  const flag = typeof location !== "undefined" ? new URLSearchParams(location.search).get("model") : null;
  let ready = flag !== "missing" && flag !== "managed";
  let updCb: (p: AppUpdateProgress) => void = () => {};
  let dlCb: (p: DownloadProgress) => void = () => {};
  let capCb: (c: Caption) => void = () => {};
  const stamp = () => {
    const s = Math.floor((Date.now() - t0) / 1000);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(Math.floor(s / 3600))}:${p(Math.floor(s / 60) % 60)}:${p(s % 60)}`;
  };
  const run = () => {
    let step = 0;
    timer = setInterval(() => {
      const me = id % 3 !== 1;
      const full = me ? "This is a synthetic line from the mock." : "And a synthetic reply from the other side.";
      const words = full.split(" ");
      step += 1;
      // Partial text grows for a few ticks, then settles into the committed line.
      if (step < 4) {
        capCb({ speaker: me ? "Me" : "Others", text: words.slice(0, Math.ceil((words.length * step) / 4)).join(" "), stable: Math.max(0, Math.ceil((words.length * step) / 4) - 2), final: false, utt: id });
        return;
      }
      step = 0;
      capCb({ speaker: me ? "Me" : "Others", text: full, stable: words.length, final: true, utt: id });
      lineCb({ kind: "statement", id: id++, t: stamp(), speaker: me ? "Me" : "Others", text: full });
    }, 450);
  };
  return {
    start: async () => {
      t0 = Date.now();
      id = 0;
      statusCb("transcribing");
      run();
    },
    pause: async () => {
      clearInterval(timer);
      lineCb({ kind: "event", id: id++, t: stamp(), text: "Transcription paused" });
      statusCb("paused");
    },
    resume: async () => {
      lineCb({ kind: "event", id: id++, t: stamp(), text: "Transcription resumed" });
      statusCb("transcribing");
      run();
    },
    end: async () => {
      clearInterval(timer);
      statusCb("ready");
    },
    rename: async () => {},
    setSource: async () => {},
    listDevices: async (source) =>
      source === "mic" ? ["Built-in microphone", "USB headset"] : ["Speakers", "USB headset"],
    setDevice: async () => {},
    modelStatus: async () => ({
      ready,
      downloadEnabled: flag !== "managed",
      updateCheckEnabled: flag !== "managed",
      customSource: false,
      busy: false,
    }),
    modelCheckUpdate: async () => ({ available: flag === "update", packs: [] }),
    appUpdateStatus: async () => ({
      installed: "0.0.1",
      mode: "notify",
      channel: upd.channel,
      repoUrl: "https://github.com/ConniptionFit/SprkNotes",
      modeLocked: flag === "managed",
      channelLocked: flag === "managed",
      repoLocked: flag === "managed",
      busy: false,
      readyToInstall: false,
      managedInstall: false,
    }),
    appUpdateCheck: async () => ({ available: upd.channel !== "stable", version: "0.2.0-beta.1" }),
    appUpdateSet: async ({ channel }) => {
      if (channel) upd.channel = channel as typeof upd.channel;
    },
    appUpdateInstall: async () => "0.2.0-beta.1",
    appUpdateNow: async () => {
      for (let pct = 0; pct <= 100; pct += 20) {
        updCb({ stage: "download", percent: pct });
        await new Promise((r) => setTimeout(r, 300));
      }
      updCb({ stage: "install" });
      return "0.2.0-beta.1";
    },
    onAppUpdateProgress: async (cb) => ((updCb = cb), () => {}),
    modelDownload: async () => {
      const total = 640_000_000;
      let done = 0;
      dlCb({ stage: "checking", done: 0 });
      const t = setInterval(() => {
        done += 40_000_000;
        if (done < total) dlCb({ stage: "downloading", pack: "Speech model", done, total });
        else {
          clearInterval(t);
          dlCb({ stage: "verifying", done: total, total });
          setTimeout(() => ((ready = true), dlCb({ stage: "done", done: total, total })), 700);
        }
      }, 250);
    },
    onModelDownload: async (cb) => ((dlCb = cb), () => {}),
    onModelUpdate: async () => () => {},
    miniResize: async () => {},
    // "?flip" lets the browser preview show the drawer above the pill.
    miniFlipped: async () => typeof location !== "undefined" && location.search.includes("flip"),
    onMiniFlip: async () => () => {},
    // Preview the question with ?mic in the overlay's address.
    micPromptPending: async () =>
      typeof location !== "undefined" && location.search.includes("mic")
        ? { up: true, apps: [{ id: "zoom.exe", name: "Zoom" }] }
        : NO_ASK,
    onMicPrompt: async () => () => {},
    micPromptAnswer: async () => {},
    setMicPrompt: async () => {},
    setHotkey: async (enabled, keys) => (enabled ? keys : ""),
    setCaptions: async () => {},
    setBrand: async () => {},
    onCaption: async (cb) => ((capCb = cb), () => {}),
    onLevels: async () => () => {},
    openApp: async () => {},
    libraryList: async (q) => mockLibrary.filter((e) => (e.title + e.name).toLowerCase().includes(q.trim().toLowerCase())),
    libraryOpen: async (path) => ({
      title: mockLibrary.find((e) => e.path === path)?.title ?? "Transcript",
      date: "2026-11-19",
      start: "09:00",
      duration: "0:32:10",
      context: "",
      lines: [
        { kind: "statement", t: "00:00:04", speaker: "Me", text: "Thanks for joining, let's start with the budget." },
        { kind: "statement", t: "00:00:11", speaker: "Others", text: "Sure. The numbers are in the shared sheet." },
        { kind: "event", t: "00:12:30", speaker: "", text: "Transcription paused" },
        { kind: "statement", t: "00:12:52", speaker: "Me", text: "Okay, back. Next item is the roadmap." },
      ],
    }),
    libraryReveal: async () => {},
    libraryOpenFolder: async () => {},
    libraryClose: async () => {},
    retention: async () => ({ ...mockRetention }),
    setRetention: async (days) => {
      if (mockRetention.locked) throw "Your organization sets how long transcripts are kept.";
      Object.assign(mockRetention, { days, source: "user" });
      return { ...mockRetention };
    },
    onRetentionRemoved: async () => () => {},
    continueTranscript: async (path) => {
      if (!mockLibrary.some((e) => e.path === path)) throw "That transcript is not in your library.";
      t0 = Date.now() - 600_000;
      id = 0;
      statusCb("transcribing");
      run();
    },
    saveLocation: async () => ({ ...mockSave }),
    managedPrefs: async () => mockManaged,
    setSaveLocation: async (path) => {
      if (path && !/^([a-zA-Z]:\\|\\\\|%)/.test(path)) throw "Enter a full folder path, for example D:\\Transcripts.";
      mockSave.chosen = path;
      mockSave.path = path ?? "C:\\Users\\you\\Documents\\SprkNotes\\Transcripts";
      mockSave.source = path ? "user" : "default";
      return { ...mockSave };
    },
    libraryExport: async (path, format) => `${path.split("/").pop()?.replace(/\.md$/, "")}.${format}`,
    latestTranscript: async () => mockLibrary[0] ?? null,
    // "?apps=both|claude|none" previews the three Summarize outcomes in the browser.
    summarize: async (_path, target) => {
      const apps = typeof location !== "undefined" ? (new URLSearchParams(location.search).get("apps") ?? "both") : "both";
      const all = [{ id: "claude", name: "Claude" }, { id: "copilot", name: "Microsoft 365 Copilot" }];
      const have = apps === "none" ? [] : apps === "claude" ? [all[0]] : apps === "copilot" ? [all[1]] : all;
      if (target) return { choose: [], toasts: [] };
      if (have.length === 0) return { choose: [], toasts: ["No Claude or Copilot app found. Prompt copied and the file is highlighted. Open your assistant, attach the file and paste."] };
      if (have.length === 1) return { choose: [], toasts: have[0].id === "claude" ? [] : ["Prompt copied. Attach the highlighted file and paste."] };
      return { choose: have, toasts: [] };
    },
    libraryFolders: async () => [...mockFolders],
    libraryRename: async (path, title) => {
      const e = mockLibrary.find((x) => x.path === path);
      const clean = title.trim().replace(/\s+/g, " ");
      if (!e || !clean) throw "That name can't be used. Try a shorter one without symbols.";
      const stamp = e.name.match(/^\d{4}-\d{2}-\d{2} \d{4} /)?.[0] ?? "";
      e.title = clean;
      e.name = stamp + clean;
      e.path = `/mock/${e.folder ? e.folder + "/" : ""}${e.name}.md`;
      return e.path;
    },
    libraryMove: async (path, folder) => {
      const e = mockLibrary.find((x) => x.path === path);
      if (!e) throw "That transcript is not in your library.";
      if (folder && !mockFolders.includes(folder)) throw "That folder is not in your library.";
      e.folder = folder;
      e.path = `/mock/${folder ? folder + "/" : ""}${e.name}.md`;
      return e.path;
    },
    libraryDelete: async (path) => {
      const i = mockLibrary.findIndex((x) => x.path === path);
      if (i >= 0) mockLibrary.splice(i, 1);
    },
    folderCreate: async (parent, name) => {
      const clean = name.trim();
      if (!clean || clean.startsWith(".") || /[\\/:<>"|?*]/.test(clean)) throw "That name can't be used. Try a shorter one without symbols.";
      const path = parent ? `${parent}/${clean}` : clean;
      if (mockFolders.some((f) => f.toLowerCase() === path.toLowerCase())) throw "There is already a folder with that name.";
      mockFolders.push(path);
      return path;
    },
    folderRename: async (folder, name) => {
      const clean = name.trim();
      if (!clean || clean.startsWith(".") || /[\\/:<>"|?*]/.test(clean)) throw "That name can't be used. Try a shorter one without symbols.";
      const parent = folder.includes("/") ? folder.slice(0, folder.lastIndexOf("/")) : "";
      const to = parent ? `${parent}/${clean}` : clean;
      const move = (p: string) => (p === folder ? to : p.startsWith(folder + "/") ? to + p.slice(folder.length) : p);
      mockFolders.splice(0, mockFolders.length, ...mockFolders.map(move));
      for (const e of mockLibrary) {
        e.folder = move(e.folder);
        e.path = `/mock/${e.folder ? e.folder + "/" : ""}${e.name}.md`;
      }
      return to;
    },
    // "?recover" shows the unexpected-close prompt in the browser preview.
    recoveryScan: async () =>
      typeof location !== "undefined" && location.search.includes("recover")
        ? [{ ...mockLibrary[0], name: "2026-11-19 1100 Team sync", title: "Team sync", status: "in-progress", duration: "" }]
        : [],
    recoveryMark: async () => {},
    onProblem: async () => () => {},
    ...mockAssistant(),
    onLine: async (cb) => ((lineCb = cb), () => {}),
    onDropped: async () => () => {},
    onStatus: async (cb) => ((statusCb = cb), () => {}),
    onPhase: async () => () => {},
  };
}

export const backend: Promise<Backend> = inTauri ? tauri() : Promise.resolve(mockBackend());

import type { AppUpdateInfo, AppUpdateStatus, AssistantRequest, AssistantResponse, AssistantStatus, Caption, DownloadProgress, ExportFormat, LibraryEntry, Line, ModelStatus, Source, Status, Summarized, TranscriptView, UpdateInfo } from "./types";

/** Typed surface between the UI and the Rust side. Every command has a matching Tauri command. */
export interface Backend {
  start(name: string): Promise<void>;
  pause(): Promise<void>;
  resume(): Promise<void>;
  end(): Promise<void>;
  rename(name: string): Promise<void>;
  setSource(source: Source, on: boolean): Promise<void>;
  modelStatus(): Promise<ModelStatus>;
  modelCheckUpdate(): Promise<UpdateInfo>;
  modelDownload(): Promise<void>;
  /** App updates (ADR-011). Policy-locked settings refuse changes with a short sentence. */
  appUpdateStatus(): Promise<AppUpdateStatus>;
  appUpdateCheck(): Promise<AppUpdateInfo>;
  appUpdateSet(change: { channel?: string; repoUrl?: string; mode?: string }): Promise<void>;
  /** Downloads and verifies; installs when the app closes. Resolves to the new version, or null. */
  appUpdateInstall(): Promise<string | null>;
  onModelDownload(cb: (p: DownloadProgress) => void): Promise<() => void>;
  onModelUpdate(cb: (u: UpdateInfo) => void): Promise<() => void>;
  /** Overlay only: size the floating window to its content (logical pixels). */
  miniResize(width: number, height: number, flipped: boolean): Promise<void>;
  /** Overlay only: true when the caption drawer should sit above the pill (pill low on screen). */
  miniFlipped(): Promise<boolean>;
  /** Overlay only: the host moved the pill to the other half of the screen. */
  onMiniFlip(cb: (flipped: boolean) => void): Promise<() => void>;
  /** Tell the engine which caption sources are wanted, so unused ones cost nothing. */
  setCaptions(on: boolean, me: boolean): Promise<void>;
  /** Swap the tray and window icons between the current brand and Throwback Mode. */
  setBrand(throwback: boolean): Promise<void>;
  /** Partial caption text while someone is still speaking (never saved). */
  onCaption(cb: (c: Caption) => void): Promise<() => void>;
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
  onLine(cb: (l: Line) => void): Promise<() => void>;
  onStatus(cb: (s: Status) => void): Promise<() => void>;
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
    modelStatus: () => invoke("model_status"),
    modelCheckUpdate: () => invoke("model_check_update"),
    modelDownload: () => invoke("model_download"),
    appUpdateStatus: () => invoke("update_status"),
    appUpdateCheck: () => invoke("update_check"),
    appUpdateSet: ({ channel, repoUrl, mode }) => invoke("update_set", { channel, repoUrl, mode }),
    appUpdateInstall: () => invoke("update_install"),
    onModelDownload: (cb) => listen<DownloadProgress>("model-download", (e) => cb(e.payload)),
    onModelUpdate: (cb) => listen<UpdateInfo>("model-update", (e) => cb(e.payload)),
    miniResize: (width, height, flipped) => invoke("mini_resize", { width, height, flipped }),
    miniFlipped: () => invoke<boolean>("mini_flipped"),
    onMiniFlip: (cb) => listen<boolean>("mini-flip", (e) => cb(e.payload)),
    setCaptions: (on, me) => invoke("set_captions", { on, me }),
    setBrand: (throwback) => invoke("set_brand", { throwback }),
    onCaption: (cb) => listen<Caption>("caption", (e) => cb(e.payload)),
    openApp: () => invoke("open_app"),
    libraryList: (query) => invoke("library_list", { query }),
    libraryOpen: (path) => invoke("library_open", { path }),
    libraryReveal: (path) => invoke("library_reveal", { path }),
    libraryExport: (path, format) => invoke("library_export", { path, format }),
    latestTranscript: () => invoke("latest_transcript"),
    summarize: (path, target) => invoke("summarize", { path, target }),
    recoveryScan: () => invoke("recovery_scan"),
    recoveryMark: (path) => invoke("recovery_mark", { path }),
    onProblem: (cb) => listen<string>("session-problem", (e) => cb(e.payload)),
    assistantStatus: () => invoke("assistant_status"),
    assistantSet: (enabled, tier, features) => invoke("assistant_set", { enabled, tier, features }),
    assistantCancel: () => invoke("assistant_cancel"),
    assistantRun: (request) => invoke("assistant_run", { request }),
    onLine: (cb) => listen<Line>("transcript-line", (e) => cb(e.payload)),
    onStatus: (cb) => listen<Status>("session-status", (e) => cb(e.payload)),
  };
}

const mockLibrary: LibraryEntry[] = [
  { path: "/mock/2026-11-19 0900 Budget sync.md", name: "2026-11-19 0900 Budget sync", title: "Budget sync", date: "2026-11-19", start: "09:00", duration: "0:32:10", status: "complete", folder: "" },
  { path: "/mock/2026-11-18 1400 Roadmap review.md", name: "2026-11-18 1400 Roadmap review", title: "Roadmap review", date: "2026-11-18", start: "14:00", duration: "0:48:02", status: "complete", folder: "" },
];

const CORE = ["title", "recap", "actions", "ask"] as const;

/** Synthetic Ask Sprk answers for the browser preview. "?assistant=nomodel|memory" previews the other states. */
function mockAssistant(): Pick<Backend, "assistantStatus" | "assistantSet" | "assistantCancel" | "assistantRun"> {
  const flag = typeof location !== "undefined" ? new URLSearchParams(location.search).get("assistant") : null;
  let on = false;
  let features: string[] = [...CORE];
  let pins: { lineId: number; t: string; label: string }[] = [];
  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  return {
    assistantStatus: async () => ({
      state: !on ? "off" : flag === "nomodel" ? "noModel" : flag === "memory" ? "notEnoughMemory" : "ready",
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
      repoUrl: "https://git.pwrs.dev/powers/sprknotes",
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
    setCaptions: async () => {},
    setBrand: async () => {},
    onCaption: async (cb) => ((capCb = cb), () => {}),
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
    // "?recover" shows the unexpected-close prompt in the browser preview.
    recoveryScan: async () =>
      typeof location !== "undefined" && location.search.includes("recover")
        ? [{ ...mockLibrary[0], name: "2026-11-19 1100 Team sync", title: "Team sync", status: "in-progress", duration: "" }]
        : [],
    recoveryMark: async () => {},
    onProblem: async () => () => {},
    ...mockAssistant(),
    onLine: async (cb) => ((lineCb = cb), () => {}),
    onStatus: async (cb) => ((statusCb = cb), () => {}),
  };
}

export const backend: Promise<Backend> = inTauri ? tauri() : Promise.resolve(mockBackend());

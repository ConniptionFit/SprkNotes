import type { Caption, DownloadProgress, ExportFormat, LibraryEntry, Line, ModelStatus, Source, Status, Summarized, TranscriptView, UpdateInfo } from "./types";

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
    onLine: (cb) => listen<Line>("transcript-line", (e) => cb(e.payload)),
    onStatus: (cb) => listen<Status>("session-status", (e) => cb(e.payload)),
  };
}

const mockLibrary: LibraryEntry[] = [
  { path: "/mock/2026-11-19 0900 Budget sync.md", name: "2026-11-19 0900 Budget sync", title: "Budget sync", date: "2026-11-19", start: "09:00", duration: "0:32:10", status: "complete", folder: "" },
  { path: "/mock/2026-11-18 1400 Roadmap review.md", name: "2026-11-18 1400 Roadmap review", title: "Roadmap review", date: "2026-11-18", start: "14:00", duration: "0:48:02", status: "complete", folder: "" },
];

/** Browser-only stand-in with synthetic text, so the UI can be built and checked without Tauri. */
export function mockBackend(): Backend {
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
    onLine: async (cb) => ((lineCb = cb), () => {}),
    onStatus: async (cb) => ((statusCb = cb), () => {}),
  };
}

export const backend: Promise<Backend> = inTauri ? tauri() : Promise.resolve(mockBackend());

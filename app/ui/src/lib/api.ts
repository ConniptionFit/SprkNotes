import type { DownloadProgress, Line, ModelStatus, Source, Status, UpdateInfo } from "./types";

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
  /** Mini controller only: grow or shrink the floating window. */
  miniExpand(expanded: boolean): Promise<void>;
  /** Mini controller only: bring the main window back. */
  openApp(): Promise<void>;
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
    miniExpand: (expanded) => invoke("mini_expand", { expanded }),
    openApp: () => invoke("open_app"),
    onLine: (cb) => listen<Line>("transcript-line", (e) => cb(e.payload)),
    onStatus: (cb) => listen<Status>("session-status", (e) => cb(e.payload)),
  };
}

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
  const stamp = () => {
    const s = Math.floor((Date.now() - t0) / 1000);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(Math.floor(s / 3600))}:${p(Math.floor(s / 60) % 60)}:${p(s % 60)}`;
  };
  const run = () => {
    timer = setInterval(() => {
      const me = id % 3 !== 1;
      lineCb({
        kind: "statement",
        id: id++,
        t: stamp(),
        speaker: me ? "Me" : "Others",
        text: me ? "This is a synthetic line from the mock." : "And a synthetic reply from the other side.",
      });
    }, 1500);
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
    miniExpand: async () => {},
    openApp: async () => {},
    onLine: async (cb) => ((lineCb = cb), () => {}),
    onStatus: async (cb) => ((statusCb = cb), () => {}),
  };
}

export const backend: Promise<Backend> = inTauri ? tauri() : Promise.resolve(mockBackend());

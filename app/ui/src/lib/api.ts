import type { Caption, DownloadProgress, Line, ModelStatus, Source, Status, UpdateInfo } from "./types";

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
  miniResize(width: number, height: number): Promise<void>;
  /** Tell the engine which caption sources are wanted, so unused ones cost nothing. */
  setCaptions(on: boolean, me: boolean): Promise<void>;
  /** Partial caption text while someone is still speaking (never saved). */
  onCaption(cb: (c: Caption) => void): Promise<() => void>;
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
    miniResize: (width, height) => invoke("mini_resize", { width, height }),
    setCaptions: (on, me) => invoke("set_captions", { on, me }),
    onCaption: (cb) => listen<Caption>("caption", (e) => cb(e.payload)),
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
        capCb({ speaker: me ? "Me" : "Others", text: words.slice(0, Math.ceil((words.length * step) / 4)).join(" "), stable: Math.max(0, Math.ceil((words.length * step) / 4) - 2), final: false });
        return;
      }
      step = 0;
      capCb({ speaker: me ? "Me" : "Others", text: full, stable: words.length, final: true });
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
    setCaptions: async () => {},
    onCaption: async (cb) => ((capCb = cb), () => {}),
    openApp: async () => {},
    onLine: async (cb) => ((lineCb = cb), () => {}),
    onStatus: async (cb) => ((statusCb = cb), () => {}),
  };
}

export const backend: Promise<Backend> = inTauri ? tauri() : Promise.resolve(mockBackend());

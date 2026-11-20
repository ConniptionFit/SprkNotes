<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./lib/api";
  import type { Caption, DownloadProgress, HandoffTarget, Line, ModelStatus, Source, Status, Summarized } from "./lib/types";
  import TopBar from "./lib/TopBar.svelte";
  import Controls from "./lib/Controls.svelte";
  import LivePane from "./lib/LivePane.svelte";
  import Footer from "./lib/Footer.svelte";
  import Settings from "./lib/Settings.svelte";
  import Library from "./lib/Library.svelte";
  import ModelDownload from "./lib/ModelDownload.svelte";
  import { captionFlags } from "./lib/prefs.svelte";

  let api = $state.raw<Backend | undefined>();
  let name = $state("");
  let status = $state<Status>("ready");
  let mic = $state(true);
  let system = $state(true);
  let lines = $state<Line[]>([]);
  let view = $state<"main" | "settings" | "library">("main");
  // Summarize: the transcript it will use, a pending choice between apps, and a short notice.
  let hasTranscript = $state(false);
  let choose = $state<{ path: string | null; targets: HandoffTarget[] } | null>(null);
  let notice = $state("");
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  // Assume a model is present until the app says otherwise, so Start never flickers disabled.
  let model = $state<ModelStatus>({ ready: true, downloadEnabled: true, updateCheckEnabled: false, customSource: false, busy: false });
  let progress = $state<DownloadProgress | null>(null);
  let updateAvailable = $state(false);
  let checkMessage = $state("");
  let partial = $state<Caption | null>(null);

  // Ask the engine for partial captions whenever either window wants them.
  $effect(() => {
    api?.setCaptions(...captionFlags()).catch(() => {});
  });
  $effect(() => {
    if (status === "ready") partial = null;
  });

  onMount(() => {
    const offs: Array<() => void> = [];
    backend.then(async (b) => {
      api = b;
      offs.push(await b.onLine((l) => (lines = [...lines, l])));
      offs.push(
        await b.onCaption((c) => {
          if (c.final) {
            if (partial?.speaker === c.speaker) partial = null;
          } else partial = c;
        }),
      );
      offs.push(await b.onStatus((s) => (status = s)));
      offs.push(
        await b.onModelDownload(async (p) => {
          progress = p;
          if (p.stage === "done" || p.stage === "error") {
            model = await b.modelStatus();
            if (p.stage === "done") updateAvailable = false;
          }
        }),
      );
      offs.push(await b.onModelUpdate((u) => (updateAvailable = u.available)));
      model = await b.modelStatus();
    });
    return () => offs.forEach((f) => f());
  });

  async function download() {
    checkMessage = "";
    progress = { stage: "checking", done: 0 };
    try {
      await api?.modelDownload();
    } catch (e) {
      progress = { stage: "error", done: 0, error: String(e) };
    }
  }

  async function check() {
    checkMessage = "Checking…";
    try {
      const r = await api?.modelCheckUpdate();
      updateAvailable = !!r?.available;
      checkMessage = r?.available ? "" : "You have the latest model.";
    } catch (e) {
      checkMessage = String(e);
    }
  }

  async function refreshLatest() {
    try {
      hasTranscript = !!(await api?.latestTranscript());
    } catch {
      hasTranscript = false;
    }
  }
  // Re-check when the app is ready and whenever a session ends or the library is left.
  $effect(() => {
    void api;
    void status;
    void view;
    if (status === "ready") refreshLatest();
  });

  function say(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = ""), 9000);
  }

  async function summarize(path: string | null, target: string | null = null) {
    choose = null;
    try {
      const r: Summarized | undefined = await api?.summarize(path, target);
      if (!r) return;
      if (r.clipboard) {
        // No clipboard tool on this system: the window copies the prompt itself.
        try {
          await navigator.clipboard.writeText(r.clipboard);
        } catch {
          r.toasts.push("The prompt could not be copied automatically.");
        }
      }
      if (r.choose.length > 0) choose = { path, targets: r.choose };
      else if (r.toasts.length > 0) say(r.toasts.join(" "));
    } catch (e) {
      say(String(e));
    }
  }

  async function start() {
    lines = [];
    await api?.start(name);
  }
</script>

<main>
  <h1 class="sr-only">SprkNotes</h1>
  {#if view === "library"}
    <Library {api} {notice} onback={() => (view = "main")} onsummarize={(p) => summarize(p)} />
  {:else if view === "settings"}
    <Settings
      {model}
      {progress}
      {updateAvailable}
      sessionActive={status !== "ready"}
      {checkMessage}
      ondownload={download}
      oncheck={check}
      onback={() => (view = "main")}
    />
  {:else}
    <TopBar
      bind:name
      {status}
      bind:mic
      bind:system
      onrename={(n) => api?.rename(n)}
      onsource={(s: Source, on: boolean) => api?.setSource(s, on)}
    />
    <LivePane {lines} {status} {partial} needsModel={!model.ready && status === "ready"}>
      {#snippet empty()}
        <ModelDownload
          compact
          {model}
          {progress}
          {updateAvailable}
          sessionActive={false}
          {checkMessage}
          ondownload={download}
          oncheck={check}
        />
      {/snippet}
    </LivePane>
  {/if}
  <div class="dock">
    <Footer
      onlibrary={() => (view = view === "library" ? "main" : "library")}
      libraryOpen={view === "library"}
      onsettings={() => (view = view === "settings" ? "main" : "settings")}
      settingsOpen={view === "settings"}
      onsummarize={() => summarize(null)}
      canSummarize={status === "ready" && hasTranscript}
      summarizeHint={status !== "ready" ? "Available after a session ends" : hasTranscript ? "Summarize your latest transcript" : "No transcript to summarize yet"}
    />
    <Controls
      {status}
      canStart={model.ready}
      onstart={start}
      onpause={() => api?.pause()}
      onresume={() => api?.resume()}
      onend={() => api?.end()}
    />
  </div>
  {#if choose}
    <div class="choose" role="dialog" aria-label="Choose an app">
      <p>Summarize with</p>
      {#each choose.targets as t (t.id)}
        <button onclick={() => summarize(choose?.path ?? null, t.id)}>{t.name}</button>
      {/each}
      <button class="quiet" onclick={() => (choose = null)}>Cancel</button>
    </div>
  {/if}
  {#if notice && view !== "library"}
    <p class="toast" role="status">{notice}</p>
  {/if}
</main>

<style>
  main { position: relative; }
  .choose {
    position: absolute; left: 1.5rem; bottom: 4.5rem; z-index: 10; display: grid; gap: 0.35rem; min-width: 14rem;
    padding: 0.75rem; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow);
  }
  .choose p { margin: 0 0 0.15rem; color: var(--muted); font-size: 0.9rem; }
  .choose button { padding: 0.5rem 0.8rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); text-align: left; }
  .choose button:hover { background: var(--accent-soft); }
  .choose .quiet { border-color: transparent; color: var(--muted); }
  .toast {
    position: absolute; left: 1.5rem; right: 1.5rem; bottom: 4.5rem; z-index: 10; margin: 0; padding: 0.7rem 1rem;
    border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow); font-size: 0.95rem;
  }
  main {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    height: 100vh;
    padding: 1.25rem 1.5rem 1rem;
    max-width: 62rem;
    margin: 0 auto;
  }
  .dock { display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap; }
</style>

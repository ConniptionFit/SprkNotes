<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./lib/api";
  import type { AssistantStatus, Caption, DownloadProgress, HandoffTarget, LibraryEntry, Line, ModelStatus, Phase, Source, Status, Summarized } from "./lib/types";
  import TopBar from "./lib/TopBar.svelte";
  import Controls from "./lib/Controls.svelte";
  import LivePane from "./lib/LivePane.svelte";
  import Footer from "./lib/Footer.svelte";
  import Settings from "./lib/Settings.svelte";
  import Library from "./lib/Library.svelte";
  import ModelDownload from "./lib/ModelDownload.svelte";
  import { captionFlags, prefs, setPref } from "./lib/prefs.svelte";
  import Ask from "./lib/Ask.svelte";
  import { Partials, Rows, type Row } from "./lib/live";
  import { hotkeyProblem } from "./lib/hotkey";
  import { hotkeyState } from "./lib/hotkeyState.svelte";

  let api = $state.raw<Backend | undefined>();
  let name = $state("");
  let status = $state<Status>("ready");
  let phase = $state<Phase>({ phase: "idle" });
  // Which sources are on is remembered across launches (prefs), and applied before the first Start.
  let mic = $state(prefs.micOn);
  let system = $state(prefs.systemOn);
  let lines = $state<Line[]>([]);
  let view = $state<"main" | "settings" | "library">("main");
  // Summarize: the transcript it will use, a pending choice between apps, and a short notice.
  let hasTranscript = $state(false);
  let choose = $state<{ path: string | null; targets: HandoffTarget[] } | null>(null);
  let notice = $state("");
  // Transcripts a crash left unfinished, offered once at launch (SPEC 6.4).
  let recovered = $state<LibraryEntry[]>([]);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  // Assume a model is present until the app says otherwise, so Start never flickers disabled.
  let model = $state<ModelStatus>({ ready: true, downloadEnabled: true, updateCheckEnabled: false, customSource: false, busy: false });
  let progress = $state<DownloadProgress | null>(null);
  let updateAvailable = $state(false);
  let checkMessage = $state("");
  // Speech in progress, one caption per speaker (each gets its own live row).
  const speaking = new Partials();
  let partials = $state<Caption[]>([]);
  // Input levels for the Me and Others buttons, while transcribing.
  let levels = $state<[number, number]>([0, 0]);
  // Ask Sprk: off by default. The panel and the title suggestion only exist when it is ready.
  let assistant = $state<AssistantStatus | null>(null);
  let askOpen = $state(false);
  let titleIdea = $state("");
  let titleBusy = $state(false);
  // Sentences that locked in the live view until their committed line arrives (ADR-008).
  const locked = new Rows();
  let liveRows = $state<Row[]>([]);

  // Ask the engine for partial captions whenever either window wants them.
  $effect(() => {
    api?.setCaptions(...captionFlags()).catch(() => {});
  });
  // The background shortcut (SPEC 8.6). Only this window registers it.
  $effect(() => {
    const on = prefs.hotkeyOn;
    const keys = prefs.hotkeyKeys;
    if (!api) return;
    api
      .setHotkey(on, keys)
      .then(() => (hotkeyState.problem = ""))
      .catch((e) => (hotkeyState.problem = hotkeyProblem(e)));
  });
  // "Start a transcript?" settings (SPEC 8.3a): on or off, and the apps not to ask about.
  $effect(() => {
    const ids = prefs.micPromptSkip.map((a) => a.id);
    api?.setMicPrompt(prefs.micPromptOn, ids).catch(() => {});
  });
  $effect(() => {
    if (status === "ready") {
      speaking.clear();
      partials = [];
      levels = [0, 0];
      locked.clear();
      liveRows = [];
    }
  });

  onMount(() => {
    const offs: Array<() => void> = [];
    backend.then(async (b) => {
      api = b;
      // Apply the saved device picks before the first session.
      b.setDevice("mic", prefs.micDevice).catch(() => {});
      b.setDevice("system", prefs.systemDevice).catch(() => {});
      // The shell keeps which sources are off between sessions; start from the remembered choice.
      b.setSource("mic", mic).catch(() => {});
      b.setSource("system", system).catch(() => {});
      offs.push(
        await b.onLine((l) => {
          lines = [...lines, l];
          if (l.kind === "statement") {
            locked.line(l.speaker, l.text);
            liveRows = [...locked.rows];
          }
        }),
      );
      offs.push(
        await b.onDropped((d) => {
          locked.dropped(d.speaker, d.text);
          liveRows = [...locked.rows];
        }),
      );
      offs.push(
        await b.onCaption((c) => {
          locked.caption(c, Date.now());
          liveRows = [...locked.rows];
          speaking.caption(c);
          partials = [...speaking.list];
        }),
      );
      offs.push(await b.onLevels((l) => (levels = l)));
      offs.push(await b.onStatus((s) => (status = s)));
      offs.push(await b.onPhase((p) => (phase = p)));
      offs.push(await b.onProblem((kind) => say(problemText(kind))));
      b.recoveryScan()
        .then((r) => (recovered = r))
        .catch(() => {});
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
      await syncAssistant();
    });
    return () => offs.forEach((f) => f());
  });

  function setSource(s: Source, on: boolean) {
    setPref(s === "mic" ? "micOn" : "systemOn", on);
    api?.setSource(s, on).catch(() => {});
  }

  async function syncAssistant() {
    try {
      await api?.assistantSet(prefs.assistantOn, prefs.assistantTier, prefs.assistantFeatures);
      assistant = (await api?.assistantStatus()) ?? null;
    } catch {
      assistant = null;
    }
  }
  // Settings changes arrive through the saved preferences; look again when leaving Settings.
  $effect(() => {
    if (view !== "settings") syncAssistant();
  });
  const askReady = $derived(!!assistant && assistant.enabled && assistant.state === "ready");
  // The Ask button shows only while the assistant and its Ask feature are on (Settings turns them on).
  const askAvailable = $derived(!!assistant && assistant.enabled && assistant.features.includes("ask"));
  function onAsk() {
    askOpen = !askOpen;
  }
  // Turning Ask Sprk off closes its panel too.
  $effect(() => {
    if (!askAvailable) askOpen = false;
  });
  const canTitle = $derived(askReady && !!assistant?.features.includes("title") && status !== "ready" && !name.trim() && lines.length >= 6);

  async function suggestTitle() {
    if (!api || titleBusy) return;
    titleBusy = true;
    titleIdea = "";
    try {
      const r = await api.assistantRun({ op: "title" });
      if (r.kind === "title" && r.text) titleIdea = r.text;
      else say("Not enough has been said to suggest a title yet.");
    } catch {
      say("A title could not be suggested right now.");
    } finally {
      titleBusy = false;
    }
  }

  function useTitle() {
    name = titleIdea;
    titleIdea = "";
    api?.rename(name);
  }

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

  function problemText(kind: string): string {
    if (kind === "write_failed") return "A line could not be saved to your transcript. Check that the folder can be written to and has free space.";
    if (kind === "finalize_failed") return "The transcript could not be closed properly. It still holds everything saved so far.";
    return "Something went wrong with the session.";
  }

  async function start() {
    lines = [];
    phase = { phase: "loading" }; // instant feedback; the backend adds the model's name
    try {
      await api?.start(name);
    } catch (e) {
      say(String(e));
    } finally {
      phase = { phase: "idle" };
    }
  }

  // Library "Continue transcribing": the same file gets the new lines, after a "continued" note.
  async function continueFrom(entry: LibraryEntry) {
    view = "main";
    lines = [];
    name = entry.title || entry.name;
    phase = { phase: "loading" };
    try {
      await api?.continueTranscript(entry.path);
    } catch (e) {
      say(String(e));
    } finally {
      phase = { phase: "idle" };
    }
  }

  async function end() {
    phase = { phase: "ending" };
    try {
      await api?.end();
    } catch (e) {
      say(String(e));
    } finally {
      phase = { phase: "idle" };
    }
  }

  async function markFinished() {
    const all = recovered;
    recovered = [];
    try {
      for (const r of all) await api?.recoveryMark(r.path);
    } catch (e) {
      say(String(e));
    }
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape" && choose) choose = null; }} />

<main>
  <h1 class="sr-only">SprkNotes</h1>
  {#if recovered.length > 0 && view === "main" && status === "ready"}
    <div class="recover" role="alert">
      <p>
        SprkNotes closed unexpectedly. {recovered.length === 1 ? "The transcript up to that point was saved." : `${recovered.length} transcripts up to that point were saved.`}
      </p>
      <div class="actions">
        <button
          onclick={() => {
            recovered = [];
            view = "library";
          }}>Open</button>
        <button class="quiet" onclick={markFinished}>Mark finished</button>
      </div>
    </div>
  {/if}
  {#if view === "library"}
    <Library {api} {notice} onback={() => (view = "main")} onsummarize={(p) => summarize(p)} oncontinue={continueFrom} canContinue={status === "ready"} />
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
      levels={status === "transcribing" ? levels : null}
      onrename={(n) => api?.rename(n)}
      onsource={setSource}
    />
    {#if canTitle}
      <div class="titleidea">
        {#if titleIdea}
          <span>Title idea: <b>{titleIdea}</b></span>
          <button onclick={useTitle}>Use it</button>
          <button class="quiet" onclick={() => (titleIdea = "")}>No thanks</button>
        {:else}
          <button class="quiet" disabled={titleBusy} onclick={suggestTitle}>{titleBusy ? "Thinking…" : "Suggest a title"}</button>
        {/if}
      </div>
    {/if}
    <LivePane {lines} {status} {partials} rows={liveRows} needsModel={!model.ready && status === "ready"}>
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
      onhome={() => {
        view = "main";
        choose = null;
      }}
      homeOpen={view === "main"}
      onlibrary={() => (view = view === "library" ? "main" : "library")}
      libraryOpen={view === "library"}
      onsettings={() => (view = view === "settings" ? "main" : "settings")}
      settingsOpen={view === "settings"}
      onsummarize={() => summarize(null)}
      onask={askAvailable ? onAsk : undefined}
      {askOpen}
      canSummarize={status === "ready" && hasTranscript}
      summarizeHint={status !== "ready" ? "Available after a session ends" : hasTranscript ? "Summarize your latest transcript" : "No transcript to summarize yet"}
    />
    <Controls
      {status}
      canStart={model.ready}
      onstart={start}
      onpause={() => api?.pause()}
      onresume={() => api?.resume()}
      onend={end}
      {phase}
    />
  </div>
  {#if askOpen && assistant?.enabled && view === "main"}
    <Ask
      {api}
      {lines}
      features={assistant?.features ?? []}
      status={assistant}
      onsettings={() => ((askOpen = false), (view = "settings"))}
      onclose={() => (askOpen = false)}
    />
  {/if}
  {#if choose}
    <button class="scrim" aria-label="Close menu" tabindex="-1" onclick={() => (choose = null)}></button>
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
  .scrim { position: fixed; inset: 0; z-index: 9; border: 0; background: transparent; padding: 0; cursor: default; }
  .choose {
    position: absolute; left: clamp(1.5rem, 4vw, 4rem); bottom: 4.5rem; z-index: 10; display: grid; gap: 0.35rem; min-width: 14rem;
    padding: 0.75rem; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow);
  }
  .titleidea { display: flex; align-items: center; gap: 0.6rem; flex-wrap: wrap; }
  .titleidea button { padding: 0.35rem 0.75rem; border: 1px solid var(--border); border-radius: 999px; background: var(--surface); color: var(--text); }
  .titleidea button:hover:not(:disabled) { background: var(--accent-soft); }
  .titleidea .quiet { color: var(--muted); }
  .recover {
    display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap;
    padding: 0.7rem 1rem; border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow);
  }
  .recover p { margin: 0; font-size: 0.95rem; }
  .recover .actions { display: flex; gap: 0.5rem; }
  .recover button { padding: 0.45rem 0.8rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); }
  .recover button:hover { background: var(--accent-soft); }
  .recover .quiet { border-color: transparent; color: var(--muted); }
  .choose p { margin: 0 0 0.15rem; color: var(--muted); font-size: 0.9rem; }
  .choose button { padding: 0.5rem 0.8rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); text-align: left; }
  .choose button:hover { background: var(--accent-soft); }
  .choose .quiet { border-color: transparent; color: var(--muted); }
  .toast {
    position: absolute; left: clamp(1.5rem, 4vw, 4rem); right: clamp(1.5rem, 4vw, 4rem); bottom: 4.5rem; z-index: 10; margin: 0; padding: 0.7rem 1rem;
    border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow); font-size: 0.95rem;
  }
  main {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    height: 100vh;
    padding: 1.25rem clamp(1.5rem, 4vw, 4rem) 1rem;
  }
  .dock { display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap; }
</style>

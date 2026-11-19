<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./lib/api";
  import type { DownloadProgress, Line, ModelStatus, Source, Status } from "./lib/types";
  import TopBar from "./lib/TopBar.svelte";
  import Controls from "./lib/Controls.svelte";
  import LivePane from "./lib/LivePane.svelte";
  import Footer from "./lib/Footer.svelte";
  import Settings from "./lib/Settings.svelte";
  import ModelDownload from "./lib/ModelDownload.svelte";

  let api: Backend | undefined;
  let name = $state("");
  let status = $state<Status>("ready");
  let mic = $state(true);
  let system = $state(true);
  let lines = $state<Line[]>([]);
  let view = $state<"main" | "settings">("main");
  // Assume a model is present until the app says otherwise, so Start never flickers disabled.
  let model = $state<ModelStatus>({ ready: true, downloadEnabled: true, updateCheckEnabled: false, customSource: false, busy: false });
  let progress = $state<DownloadProgress | null>(null);
  let updateAvailable = $state(false);
  let checkMessage = $state("");

  onMount(() => {
    const offs: Array<() => void> = [];
    backend.then(async (b) => {
      api = b;
      offs.push(await b.onLine((l) => (lines = [...lines, l])));
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

  async function start() {
    lines = [];
    await api?.start(name);
  }
</script>

<main>
  <h1 class="sr-only">SprkNotes</h1>
  {#if view === "settings"}
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
    <LivePane {lines} {status} needsModel={!model.ready && status === "ready"}>
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
    <Footer onsettings={() => (view = view === "settings" ? "main" : "settings")} settingsOpen={view === "settings"} />
    <Controls
      {status}
      canStart={model.ready}
      onstart={start}
      onpause={() => api?.pause()}
      onresume={() => api?.resume()}
      onend={() => api?.end()}
    />
  </div>
</main>

<style>
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

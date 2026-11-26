<script lang="ts">
  import { onMount } from "svelte";
  import type { Backend } from "./api";
  import type { AssistantModelStatus, DownloadProgress } from "./types";

  let {
    api,
    tier,
    sessionActive,
    onchange,
  }: {
    api: Backend | undefined;
    /** The size the user picked in Settings: titles, light or standard. */
    tier: "titles" | "light" | "standard";
    sessionActive: boolean;
    /** Called when the installed packs change, so the Ask Sprk switch can update. */
    onchange: (m: AssistantModelStatus | null) => void;
  } = $props();

  let model = $state<AssistantModelStatus | null>(null);
  let progress = $state<DownloadProgress | null>(null);
  let updateAvailable = $state(false);
  let checkMessage = $state("");

  const NAMES = { titles: "Titles only", light: "Light", standard: "Standard" } as const;
  // Rough download sizes, only so people know what they are starting.
  const SIZES = { titles: "0.5 GB", light: "1.3 GB", standard: "2.7 GB" } as const;

  const have = $derived(model?.installed.some((i) => i.tier === tier) ?? false);
  const working = $derived(
    !!model?.busy || (progress !== null && ["checking", "downloading", "verifying"].includes(progress.stage)),
  );
  const pct = $derived(
    progress?.stage === "downloading" && progress.total
      ? Math.min(100, Math.round((progress.done / progress.total) * 100))
      : null,
  );
  const mb = (n: number) => (n >= 1_000_000_000 ? `${(n / 1_000_000_000).toFixed(1)} GB` : `${Math.round(n / 1_000_000)} MB`);
  const label = $derived.by(() => {
    if (!progress) return "Working…";
    if (progress.stage === "checking") return "Looking for the model…";
    if (progress.stage === "verifying") return "Checking the download…";
    return progress.total ? `Downloading: ${mb(progress.done)} of ${mb(progress.total)}` : "Downloading…";
  });
  const failed = $derived(progress?.stage === "error" ? (progress.error ?? "The download did not finish.") : null);
  const justDone = $derived(progress?.stage === "done");

  async function refresh() {
    try {
      model = (await api?.assistantModelStatus()) ?? null;
    } catch {
      model = null;
    }
    onchange(model);
  }

  async function download() {
    checkMessage = "";
    updateAvailable = false;
    progress = { stage: "checking", done: 0 };
    try {
      await api?.assistantModelDownload(tier);
    } catch (e) {
      progress = { stage: "error", done: 0, error: String(e) };
    }
  }

  async function check() {
    checkMessage = "Checking…";
    try {
      const u = await api?.assistantModelCheck(tier);
      updateAvailable = !!u?.available;
      checkMessage = u?.available ? "" : "You have the latest model.";
    } catch (e) {
      checkMessage = String(e);
    }
  }

  onMount(() => {
    let off: (() => void)[] = [];
    let gone = false;
    (async () => {
      // The parent sets `api` after the backend resolves, so wait for it.
      while (!api && !gone) await new Promise((r) => setTimeout(r, 50));
      if (gone || !api) return;
      await refresh();
      off.push(
        await api.onAssistantModelDownload(async (p) => {
          progress = p;
          if (p.stage === "done" || p.stage === "error") await refresh();
        }),
        await api.onAssistantModelUpdate((u) => (updateAvailable = u.available)),
      );
    })();
    return () => {
      gone = true;
      off.forEach((f) => f());
    };
  });
</script>

<div class="model">
  <div class="line">
    <p class="status" class:missing={!have}>
      {#if !model}
        Checking…
      {:else if have}
        {NAMES[tier]} model installed
      {:else if model.anyInstalled}
        The {NAMES[tier]} model is not installed yet
      {:else}
        No model installed yet
      {/if}
    </p>
    {#if model?.customSource}<span class="hint">Your organization provides the download.</span>{/if}
  </div>

  {#if model && !model.downloadEnabled}
    <p class="hint">Downloads are managed by your organization.</p>
  {:else if model}
    {#if working}
      <div>
        <p id="am-label" class="hint">{label}</p>
        <div class="bar" role="progressbar" aria-labelledby="am-label" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct ?? undefined}>
          <i class:indeterminate={pct === null} style:width={pct === null ? undefined : `${pct}%`}></i>
        </div>
      </div>
    {:else}
      {#if failed}<p class="error" role="alert">{failed}</p>{/if}
      {#if justDone}<p class="ok" role="status">Model ready. You can turn on Ask Sprk.</p>{/if}
      <div class="actions">
        {#if !have || updateAvailable}
          <button class="primary" onclick={download} disabled={sessionActive} aria-describedby={sessionActive ? "am-hint" : undefined}>
            {failed ? "Try again" : have ? "Update model" : `Download ${NAMES[tier]} model`}
          </button>
          {#if !have}<span class="hint">About {SIZES[tier]}. Needs a network connection once.</span>{/if}
        {/if}
        {#if have && model.updateCheckEnabled && !updateAvailable}
          <button class="ghost" onclick={check} disabled={sessionActive}>Check for updates</button>
        {/if}
        {#if updateAvailable}<span class="badge">Update available</span>{/if}
      </div>
      {#if sessionActive}
        <p class="hint" id="am-hint">End the current session to download a model.</p>
      {:else if checkMessage}
        <p class="hint" role="status">{checkMessage}</p>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .model { display: grid; gap: 0.6rem; }
  p { margin: 0; }
  .line { display: flex; gap: 0.75rem; align-items: baseline; flex-wrap: wrap; }
  .status { font-weight: 600; color: var(--ok-text); }
  .status.missing { color: var(--warn); }
  .actions { display: flex; gap: 0.6rem; align-items: center; flex-wrap: wrap; }
  button { padding: 0.5rem 1.1rem; border-radius: 999px; border: 1px solid var(--border); background: transparent; font-weight: 600; color: var(--text); }
  button:hover:not(:disabled) { background: var(--surface-2); }
  button:disabled { opacity: 0.55; }
  .primary { background: var(--accent); border-color: transparent; color: var(--on-accent); }
  .primary:hover:not(:disabled) { background: var(--accent-hover); }
  .badge { padding: 0.2rem 0.7rem; border-radius: 999px; background: var(--accent-soft); color: var(--accent); font-size: 0.85rem; font-weight: 600; }
  .hint { color: var(--muted); font-size: 0.9rem; }
  .ok { color: var(--ok-text); font-weight: 600; }
  .error { color: var(--error-text); font-weight: 600; }
  .bar { height: 0.6rem; margin-top: 0.3rem; border-radius: 999px; background: var(--surface-2); overflow: hidden; border: 1px solid var(--border); max-width: 28rem; }
  .bar i { display: block; height: 100%; background: var(--accent); border-radius: inherit; transition: width 200ms; }
  .bar i.indeterminate { width: 35%; animation: slide 1.4s ease-in-out infinite; }
  @keyframes slide { from { margin-left: -35%; } to { margin-left: 100%; } }
  @media (forced-colors: active) { .bar i { forced-color-adjust: none; background: Highlight; } }
</style>

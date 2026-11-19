<script lang="ts">
  import type { DownloadProgress, ModelStatus } from "./types";

  let {
    model,
    progress,
    updateAvailable,
    sessionActive,
    checkMessage,
    ondownload,
    oncheck,
    compact = false,
  }: {
    model: ModelStatus;
    progress: DownloadProgress | null;
    updateAvailable: boolean;
    sessionActive: boolean;
    checkMessage: string;
    ondownload: () => void;
    oncheck: () => void;
    compact?: boolean;
  } = $props();

  const working = $derived(
    model.busy || (progress !== null && ["checking", "downloading", "verifying"].includes(progress.stage)),
  );
  const pct = $derived(
    progress?.stage === "downloading" && progress.total
      ? Math.min(100, Math.round((progress.done / progress.total) * 100))
      : null,
  );
  const mb = (n: number) => `${Math.round(n / 1_000_000)} MB`;
  const label = $derived.by(() => {
    if (!progress) return "Working…";
    switch (progress.stage) {
      case "checking": return "Checking for the model…";
      case "downloading":
        return progress.total
          ? `Downloading ${progress.pack ?? "model"}: ${mb(progress.done)} of ${mb(progress.total)}`
          : `Downloading ${progress.pack ?? "model"}…`;
      case "verifying": return "Verifying the download…";
      default: return "Working…";
    }
  });
  const justDone = $derived(progress?.stage === "done");
  const failed = $derived(progress?.stage === "error" ? (progress.error ?? "The download did not finish.") : null);
  const buttonText = $derived(model.ready ? "Update model" : "Download model");
</script>

<div class="card" class:compact>
  {#if !model.downloadEnabled}
    <p class="note">Managed by your organization</p>
  {:else}
    {#if working}
      <div class="progress">
        <p id="dl-label">{label}</p>
        <div
          class="bar"
          role="progressbar"
          aria-labelledby="dl-label"
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={pct ?? undefined}
        >
          <i class:indeterminate={pct === null} style:width={pct === null ? undefined : `${pct}%`}></i>
        </div>
      </div>
    {:else}
      {#if failed}
        <p class="error" role="alert">{failed}</p>
      {:else if justDone}
        <p class="ok" role="status">Model ready. You can start transcribing.</p>
      {/if}
      <div class="actions">
        {#if !model.ready || updateAvailable}
          <button
            class="primary"
            onclick={ondownload}
            disabled={sessionActive}
            aria-describedby={sessionActive ? "dl-hint" : undefined}
          >{failed ? "Try again" : buttonText}</button>
        {/if}
        {#if model.ready && model.updateCheckEnabled && !updateAvailable}
          <button class="ghost" onclick={oncheck} disabled={sessionActive}>Check for updates</button>
        {/if}
        {#if updateAvailable}<span class="badge">Update available</span>{/if}
      </div>
      {#if sessionActive}
        <p class="hint" id="dl-hint">End the current session to download a model.</p>
      {:else if checkMessage}
        <p class="hint" role="status">{checkMessage}</p>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .card { display: flex; flex-direction: column; gap: 0.6rem; }
  p { margin: 0; }
  .actions { display: flex; gap: 0.6rem; align-items: center; flex-wrap: wrap; }
  button { padding: 0.55rem 1.2rem; border-radius: 999px; border: 1px solid var(--border); background: transparent; font-weight: 600; }
  button:hover:not(:disabled) { background: var(--surface-2); }
  button:disabled { opacity: 0.55; }
  .primary { background: var(--accent); border-color: transparent; color: var(--on-accent); }
  .primary:hover:not(:disabled) { background: var(--accent-hover); }
  .ghost { color: var(--text); }
  .badge { padding: 0.2rem 0.7rem; border-radius: 999px; background: var(--accent-soft); color: var(--accent); font-size: 0.85rem; font-weight: 600; }
  .hint, .note { color: var(--muted); font-size: 0.9rem; }
  .ok { color: var(--ok-text); font-weight: 600; }
  .error { color: var(--error-text); font-weight: 600; }
  .bar { height: 0.6rem; border-radius: 999px; background: var(--surface-2); overflow: hidden; border: 1px solid var(--border); }
  .bar i { display: block; height: 100%; background: var(--accent); border-radius: inherit; transition: width 200ms; }
  .bar i.indeterminate { width: 35%; animation: slide 1.4s ease-in-out infinite; }
  @keyframes slide { from { margin-left: -35%; } to { margin-left: 100%; } }
  .compact { align-items: center; text-align: center; margin-top: 0.75rem; }
  .compact .progress { width: min(32rem, 100%); }
  @media (forced-colors: active) { .bar i { forced-color-adjust: none; background: Highlight; } }
</style>

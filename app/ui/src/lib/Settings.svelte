<script lang="ts">
  import type { DownloadProgress, ModelStatus } from "./types";
  import ModelDownload from "./ModelDownload.svelte";

  let {
    model,
    progress,
    updateAvailable,
    sessionActive,
    checkMessage,
    ondownload,
    oncheck,
    onback,
  }: {
    model: ModelStatus;
    progress: DownloadProgress | null;
    updateAvailable: boolean;
    sessionActive: boolean;
    checkMessage: string;
    ondownload: () => void;
    oncheck: () => void;
    onback: () => void;
  } = $props();

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") onback();
  }
</script>

<svelte:window onkeydown={key} />

<section class="settings" aria-labelledby="settings-title">
  <header>
    <button class="back" onclick={onback} aria-label="Back to transcript">
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M12 4l-6 6 6 6" /></svg>
    </button>
    <h2 id="settings-title">Settings</h2>
  </header>

  <div class="panel">
    <div class="row">
      <div class="what">
        <h3>Transcription model</h3>
        <p class="status" class:missing={!model.ready}>
          {model.ready ? "Installed and ready" : "Not installed yet"}
        </p>
        <p class="help">
          The model turns speech into text on this PC. Audio never leaves your computer.
          {#if model.customSource}Your organization provides the download.{/if}
        </p>
      </div>
      <ModelDownload
        {model}
        {progress}
        {updateAvailable}
        {sessionActive}
        {checkMessage}
        {ondownload}
        {oncheck}
      />
    </div>
  </div>
</section>

<style>
  .settings { flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 1rem; overflow-y: auto; }
  header { display: flex; align-items: center; gap: 0.5rem; }
  h2 { margin: 0; font-size: 1.6rem; font-weight: 650; letter-spacing: -0.01em; }
  h3 { margin: 0; font-size: 1.05rem; }
  .back {
    display: grid; place-items: center; width: 2.25rem; height: 2.25rem;
    border: 1px solid var(--border); border-radius: 50%; background: var(--surface);
  }
  .back:hover { background: var(--surface-2); }
  .back svg { width: 1.1rem; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .panel {
    border: 1px solid var(--border); border-radius: var(--radius);
    background: var(--surface); box-shadow: var(--shadow); padding: 1.1rem 1.25rem;
  }
  .row { display: grid; grid-template-columns: 1fr auto; gap: 1.5rem; align-items: center; }
  @media (max-width: 40rem) { .row { grid-template-columns: 1fr; } }
  .what { display: flex; flex-direction: column; gap: 0.25rem; }
  .status { margin: 0; font-weight: 600; color: var(--ok-text); }
  .status.missing { color: var(--warn); }
  .help { margin: 0; color: var(--muted); font-size: 0.9rem; max-width: 34rem; }
</style>

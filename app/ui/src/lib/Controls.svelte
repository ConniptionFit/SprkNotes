<script lang="ts">
  import type { Status } from "./types";
  let {
    status,
    onstart,
    onpause,
    onresume,
    onend,
    canStart = true,
  }: {
    status: Status;
    onstart: () => void;
    onpause: () => void;
    onresume: () => void;
    onend: () => void;
    /** False until a transcription model is installed. */
    canStart?: boolean;
  } = $props();
</script>

<div class="controls">
  {#if status === "ready"}
    <button class="primary" onclick={onstart} disabled={!canStart} title={canStart ? undefined : "Download the transcription model first"}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 4l10 6-10 6z" fill="currentColor" stroke="none" /></svg>
      Start
    </button>
  {:else}
    <button class="ghost" onclick={onend}>End</button>
    {#if status === "paused"}
      <button class="primary" onclick={onresume}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 4l10 6-10 6z" fill="currentColor" stroke="none" /></svg>
        Resume
      </button>
    {:else}
      <button class="primary" onclick={onpause}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 4h2.5v12H6zM11.5 4H14v12h-2.5z" fill="currentColor" stroke="none" /></svg>
        Pause
      </button>
    {/if}
  {/if}
</div>

<style>
  .controls { display: flex; gap: 0.5rem; align-items: center; }
  button {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 6.5rem;
    justify-content: center;
    padding: 0.65rem 1.4rem;
    border-radius: 999px;
    border: 1px solid transparent;
    font-weight: 600;
    transition: background 120ms, border-color 120ms;
  }
  svg { width: 1rem; height: 1rem; }
  .primary { background: var(--accent); color: var(--on-accent); box-shadow: var(--shadow); }
  .primary:disabled { opacity: 0.5; box-shadow: none; }
  .primary:hover:not(:disabled) { background: var(--accent-hover); }
  .ghost { background: transparent; border-color: var(--border); color: var(--text); }
  .ghost:hover { background: var(--surface-2); }
</style>

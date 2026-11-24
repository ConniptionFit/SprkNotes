<script lang="ts">
  import type { Phase, Status } from "./types";
  let {
    status,
    onstart,
    onpause,
    onresume,
    onend,
    canStart = true,
    phase = { phase: "idle" },
  }: {
    status: Status;
    onstart: () => void;
    onpause: () => void;
    onresume: () => void;
    onend: () => void;
    /** False until a transcription model is installed. */
    canStart?: boolean;
    /** Busy states: loading the model at Start, saving the transcript at End. */
    phase?: Phase;
  } = $props();
  const loading = $derived(phase.phase === "loading");
  const ending = $derived(phase.phase === "ending");
  const busy = $derived(loading || ending);
</script>

<div class="controls">
  {#if status === "ready"}
    <button class="primary" class:busy={loading} onclick={onstart} disabled={!canStart || busy} aria-busy={loading} title={canStart ? undefined : "Download the transcription model first"}>
      {#if loading}
        <span class="spin" aria-hidden="true"></span>
        <span role="status">{phase.model ? `Loading model: ${phase.model}` : "Loading model"}</span>
      {:else}
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 4l10 6-10 6z" fill="currentColor" stroke="none" /></svg>
        Start
      {/if}
    </button>
  {:else}
    <button class="ghost" class:busy={ending} onclick={onend} disabled={busy} aria-busy={ending}>
      {#if ending}
        <span class="spin" aria-hidden="true"></span>
        <span role="status">Saving transcript</span>
      {:else}
        End
      {/if}
    </button>
    {#if status === "paused"}
      <button class="primary" onclick={onresume} disabled={busy}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 4l10 6-10 6z" fill="currentColor" stroke="none" /></svg>
        Resume
      </button>
    {:else}
      <button class="primary" onclick={onpause} disabled={busy}>
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
    white-space: nowrap;
    justify-content: center;
    padding: 0.65rem 1.4rem;
    border-radius: 999px;
    border: 1px solid transparent;
    font-weight: 600;
    transition: background 120ms, border-color 120ms;
  }
  svg { width: 1rem; height: 1rem; }
  .spin {
    width: 1rem;
    height: 1rem;
    border-radius: 50%;
    border: 2px solid currentColor;
    border-right-color: transparent;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spin { animation-duration: 3s; } }
  button.busy:disabled { opacity: 1; cursor: progress; }
  .primary.busy { background: var(--accent); }
  .primary { background: var(--accent); color: var(--on-accent); box-shadow: var(--shadow); }
  .primary:disabled { opacity: 0.5; box-shadow: none; }
  .primary:hover:not(:disabled) { background: var(--accent-hover); }
  .ghost { background: transparent; border-color: var(--border); color: var(--text); }
  .ghost:hover { background: var(--surface-2); }
</style>

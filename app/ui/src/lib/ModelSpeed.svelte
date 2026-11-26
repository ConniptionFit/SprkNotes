<script lang="ts">
  // Settings: how fast the transcription model runs on this PC, from the speed check that picks
  // the decode threads (SPEC 16.8a). Read-only, with "Check again".
  import { backend } from "./api";
  import type { SpeedStatus } from "./types";
  import { canCheckSpeed, speedText } from "./speed";

  let { sessionActive = false }: { sessionActive?: boolean } = $props();

  let status = $state<SpeedStatus | null>(null);
  let running = $state(false);
  let error = $state("");
  let poll: ReturnType<typeof setTimeout> | undefined;

  async function load() {
    clearTimeout(poll);
    try {
      status = await (await backend).speedStatus();
    } catch {
      status = null;
    }
    // A check that started in the background (first launch, new model): show its result when done.
    if (status?.state === "checking") poll = setTimeout(load, 2000);
  }
  load();
  $effect(() => () => clearTimeout(poll));

  async function check() {
    running = true;
    error = "";
    status = { state: "checking", speed: null, threads: null };
    try {
      status = await (await backend).speedCheck();
    } catch (e) {
      error = String(e);
      await load();
    } finally {
      running = false;
    }
  }

  const text = $derived(speedText(status));
</script>

{#if text}
  <div class="speed">
    <p role="status">{text}</p>
    {#if canCheckSpeed(status) || running}
      <button
        class="ghost"
        onclick={check}
        disabled={running || sessionActive}
        title={sessionActive ? "End the transcript first, then check the speed." : undefined}
      >{status?.state === "none" ? "Check now" : "Check again"}</button>
    {/if}
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
{/if}

<style>
  .speed { display: flex; gap: 0.6rem; align-items: center; flex-wrap: wrap; }
  p { margin: 0; color: var(--muted); font-size: 0.9rem; }
  button { padding: 0.35rem 0.9rem; border-radius: 999px; border: 1px solid var(--border); background: transparent; color: var(--text); font-weight: 600; font-size: 0.85rem; }
  button:hover:not(:disabled) { background: var(--surface-2); }
  button:disabled { opacity: 0.55; }
  .error { color: var(--error-text); font-weight: 600; }
</style>

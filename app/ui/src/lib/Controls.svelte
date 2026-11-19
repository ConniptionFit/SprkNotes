<script lang="ts">
  import type { Status } from "./types";
  let {
    status,
    onstart,
    onpause,
    onresume,
    onend,
  }: {
    status: Status;
    onstart: () => void;
    onpause: () => void;
    onresume: () => void;
    onend: () => void;
  } = $props();
</script>

<div class="controls">
  <button onclick={onstart} disabled={status !== "ready"}>Start</button>
  {#if status === "paused"}
    <button onclick={onresume}>Resume</button>
  {:else}
    <button onclick={onpause} disabled={status !== "transcribing"}>Pause</button>
  {/if}
  <button onclick={onend} disabled={status === "ready"}>End</button>
</div>

<style>
  .controls { display: flex; gap: 0.5rem; }
  button { padding: 0.5rem 1.2rem; }
</style>

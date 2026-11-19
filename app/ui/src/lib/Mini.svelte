<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./api";
  import type { Status } from "./types";
  import Light from "./Light.svelte";

  let api: Backend | undefined;
  let status = $state<Status>("ready");
  let seconds = $state(0);
  let expanded = $state(false);
  let collapseTimer: ReturnType<typeof setTimeout> | undefined;

  const clock = $derived(
    [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60]
      .map((n) => String(n).padStart(2, "0"))
      .join(":"),
  );

  onMount(() => {
    let off: (() => void) | undefined;
    backend.then(async (b) => {
      api = b;
      off = await b.onStatus((s) => {
        if (s === "transcribing" && status === "ready") seconds = 0; // a new session
        status = s;
      });
    });
    // Elapsed time counts only while transcribing.
    const tick = setInterval(() => {
      if (status === "transcribing") seconds += 1;
    }, 1000);
    return () => {
      off?.();
      clearInterval(tick);
    };
  });

  function setExpanded(on: boolean) {
    clearTimeout(collapseTimer);
    if (on === expanded) return;
    expanded = on;
    api?.miniExpand(on);
  }
  // Expanded while the pointer is over it or a button has keyboard focus.
  let root: HTMLDivElement;
  let hovering = false;
  function refresh() {
    clearTimeout(collapseTimer);
    if (hovering || root?.matches(":focus-within")) setExpanded(true);
    // Linger briefly so a stray mouse move never makes the buttons jump away.
    else collapseTimer = setTimeout(() => setExpanded(false), 450);
  }
  const hover = (on: boolean) => ((hovering = on), refresh());
</script>

<div
  class="mini"
  bind:this={root}
  role="group"
  aria-label="SprkNotes mini controller"
  onmouseenter={() => hover(true)}
  onmouseleave={() => hover(false)}
  onfocusin={refresh}
  onfocusout={() => setTimeout(refresh)}
>
  <div class="grip" data-tauri-drag-region title="Drag to move">
    <svg viewBox="0 0 10 18" aria-hidden="true">
      <circle cx="3" cy="3" r="1.3" /><circle cx="7" cy="3" r="1.3" />
      <circle cx="3" cy="9" r="1.3" /><circle cx="7" cy="9" r="1.3" />
      <circle cx="3" cy="15" r="1.3" /><circle cx="7" cy="15" r="1.3" />
    </svg>
  </div>
  <Light {status} size={11} />
  <span class="clock" class:idle={status === "ready"}>{clock}</span>

  {#if expanded}
    <div class="actions">
      {#if status === "ready"}
        <button class="primary" onclick={() => api?.start("")}>Start</button>
      {:else}
        {#if status === "paused"}
          <button class="primary" onclick={() => api?.resume()}>Resume</button>
        {:else}
          <button class="primary" onclick={() => api?.pause()}>Pause</button>
        {/if}
        <button onclick={() => api?.end()}>End</button>
      {/if}
      <button class="open" onclick={() => api?.openApp()} title="Open SprkNotes">Open app</button>
    </div>
  {/if}
</div>

<style>
  .mini {
    display: inline-flex;
    align-items: center;
    gap: 0.6rem;
    height: 44px;
    padding: 0 0.5rem 0 0;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    box-shadow: var(--shadow);
    user-select: none;
    white-space: nowrap;
    overflow: hidden;
  }
  .grip {
    align-self: stretch;
    display: flex;
    align-items: center;
    padding: 0 0.15rem 0 0.8rem;
    color: var(--muted);
    cursor: grab;
  }
  .grip:active { cursor: grabbing; }
  .grip svg { width: 0.55rem; height: 1rem; fill: currentColor; pointer-events: none; }
  .clock { font-variant-numeric: tabular-nums; font-weight: 600; font-size: 0.9rem; min-width: 4.3rem; }
  .clock.idle { color: var(--muted); font-weight: 500; }
  .actions { display: flex; gap: 0.35rem; align-items: center; margin-left: 0.25rem; }
  button {
    padding: 0.4rem 0.9rem;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: transparent;
    font-size: 0.85rem;
    font-weight: 600;
  }
  button:hover { background: var(--surface-2); }
  .primary { background: var(--accent); border-color: transparent; color: var(--on-accent); }
  .primary:hover { background: var(--accent-hover); }
  .open { border-color: transparent; color: var(--muted); font-weight: 500; }
</style>

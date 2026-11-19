<script lang="ts">
  import { tick } from "svelte";
  import type { Line, Status } from "./types";
  import { isNearBottom, initialFollow, onScroll, onLines, returnToLive } from "./follow";

  let { lines, status = "ready" }: { lines: Line[]; status?: Status } = $props();

  let box: HTMLDivElement;
  let state = $state(initialFollow);
  let seen = 0;

  async function toBottom(smooth: boolean) {
    await tick();
    box?.scrollTo({ top: box.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  }

  $effect(() => {
    const added = lines.length - seen;
    seen = lines.length;
    if (added <= 0) return;
    if (state.follow) toBottom(true);
    else state = onLines(state, added);
  });

  function handleScroll() {
    state = onScroll(state, isNearBottom(box.scrollTop, box.clientHeight, box.scrollHeight));
  }

  function live() {
    state = returnToLive();
    toBottom(true);
  }

  function key(e: KeyboardEvent) {
    if (e.key === "End") {
      e.preventDefault();
      live();
    }
  }
</script>

<div class="wrap">
  <!-- A scrollable log must be focusable so the keyboard can scroll it and End can return to live. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="pane"
    bind:this={box}
    onscroll={handleScroll}
    onkeydown={key}
    role="log"
    aria-live="off"
    aria-label="Live transcript"
    tabindex="0"
  >
    {#each lines as l (l.id)}
      {#if l.kind === "statement"}
        <p class="line" class:me={l.speaker === "Me"} class:others={l.speaker !== "Me"}>
          <time>{l.t}</time><strong>{l.speaker}</strong><span class="text">{l.text}</span>
        </p>
      {:else}
        <p class="line event"><time>{l.t}</time><span class="text">{l.text}</span></p>
      {/if}
    {:else}
      <div class="empty">
        <svg viewBox="0 0 48 32" aria-hidden="true"><rect x="4" y="12" width="5" height="8" rx="2.5" /><rect x="13" y="6" width="5" height="20" rx="2.5" /><rect x="22" y="1" width="5" height="30" rx="2.5" /><rect x="31" y="8" width="5" height="16" rx="2.5" /><rect x="40" y="12" width="5" height="8" rx="2.5" /></svg>
        {#if status === "ready"}
          <h2>Ready when you are</h2>
          <p>Press Start and your meeting appears here as it happens.</p>
        {:else}
          <h2>Listening</h2>
          <p>The first words will show up in a moment.</p>
        {/if}
      </div>
    {/each}
  </div>
  {#if !state.follow}
    <button class="pill" onclick={live}>
      Return to live{state.unseen > 0 ? ` (${state.unseen} new)` : ""}
    </button>
  {/if}
</div>

<style>
  .wrap { position: relative; flex: 1; min-height: 0; display: flex; }
  .pane {
    flex: 1;
    overflow-y: auto;
    overflow-anchor: auto;
    padding: 0.75rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .line {
    display: grid;
    grid-template-columns: 4.6rem 4.8rem 1fr;
    align-items: baseline;
    gap: 0.25rem;
    margin: 0;
    padding: 0.45rem 0.9rem;
    border-radius: 8px;
    line-height: 1.5;
    /* Long sessions: off-screen lines skip layout and paint. */
    content-visibility: auto;
    contain-intrinsic-size: auto 2.4rem;
  }
  .line:hover { background: var(--surface-2); }
  time { color: var(--muted); font-variant-numeric: tabular-nums; font-size: 0.8rem; }
  strong { font-size: 0.85rem; font-weight: 650; }
  .me strong { color: var(--me); }
  .others strong { color: var(--others); }
  .event { color: var(--muted); font-size: 0.9rem; }
  .event .text { grid-column: 2 / -1; }
  .empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    color: var(--muted);
    text-align: center;
  }
  .empty svg { width: 4.5rem; fill: var(--accent); opacity: 0.55; margin-bottom: 0.75rem; }
  .empty h2 { margin: 0; font-size: 1.15rem; color: var(--text); }
  .empty p { margin: 0; }
  .pill {
    position: absolute;
    left: 50%;
    bottom: 1rem;
    transform: translateX(-50%);
    border: 0;
    border-radius: 999px;
    padding: 0.45rem 1.1rem;
    font-weight: 600;
    background: var(--accent);
    color: var(--on-accent);
    box-shadow: 0 4px 14px rgb(0 0 0 / 0.25);
    cursor: pointer;
  }
</style>

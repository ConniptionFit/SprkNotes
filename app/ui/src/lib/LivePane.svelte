<script lang="ts">
  import { tick } from "svelte";
  import type { Line } from "./types";
  import { isNearBottom, initialFollow, onScroll, onLines, returnToLive } from "./follow";

  let { lines }: { lines: Line[] } = $props();

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
          <time>{l.t}</time> <strong>{l.speaker}:</strong> {l.text}
        </p>
      {:else}
        <p class="line event"><time>{l.t}</time> {l.text}</p>
      {/if}
    {:else}
      <p class="empty">Press Start to begin transcribing.</p>
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
    padding: 0.75rem 1rem;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--surface);
  }
  .line {
    margin: 0.25rem 0;
    line-height: 1.45;
    /* Long sessions: off-screen lines skip layout and paint. */
    content-visibility: auto;
    contain-intrinsic-size: auto 1.45em;
  }
  time { color: var(--muted); font-variant-numeric: tabular-nums; font-size: 0.85em; }
  .me strong { color: var(--me); }
  .others strong { color: var(--others); }
  .event { color: var(--muted); font-style: italic; }
  .empty { color: var(--muted); }
  .pill {
    position: absolute;
    left: 50%;
    bottom: 1rem;
    transform: translateX(-50%);
    border: 0;
    border-radius: 999px;
    padding: 0.4rem 1rem;
    background: var(--accent);
    color: white;
    cursor: pointer;
    box-shadow: 0 2px 8px rgb(0 0 0 / 0.25);
  }
</style>

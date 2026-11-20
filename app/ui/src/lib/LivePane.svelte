<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import type { Caption, Line, Status } from "./types";
  import { prefs, setPref, LINE_TEXT } from "./prefs.svelte";
  import { isNearBottom, initialFollow, onScroll, onLines, returnToLive } from "./follow";

  let {
    lines,
    status = "ready",
    needsModel = false,
    empty,
    partial = null,
  }: { lines: Line[]; status?: Status; needsModel?: boolean; empty?: Snippet; partial?: Caption | null } = $props();

  // The live line: the sentence being spoken, growing word by word at the end of the transcript.
  const wanted = (speaker: string) => prefs.captionsWho === "both" || speaker !== "Me";
  const liveOn = $derived(prefs.liveLine && status !== "ready" && partial !== null && wanted(partial.speaker));
  const liveWords = $derived(partial ? partial.text.split(/\s+/).filter(Boolean) : []);
  // Words that just changed under the reader get a soft tint for a moment.
  let fixed = $state<Set<number>>(new Set());
  let prevWords: string[] = [];
  $effect(() => {
    const now = liveWords;
    const changed: number[] = [];
    now.forEach((w, i) => {
      if (i < prevWords.length && prevWords[i] !== w) changed.push(i);
    });
    prevWords = now;
    if (changed.length === 0) return;
    fixed = new Set([...fixed, ...changed]);
    setTimeout(() => (fixed = new Set([...fixed].filter((i) => !changed.includes(i)))), 1400);
  });
  const size = $derived(LINE_TEXT[prefs.lineText]);
  const bump = (d: number) => setPref("lineText", Math.min(LINE_TEXT.length - 1, Math.max(0, prefs.lineText + d)));

  // Keep the live line in view while following.
  $effect(() => {
    void liveWords.length;
    if (liveOn && fs.follow) toBottom(false);
  });

  let box: HTMLDivElement;
  let fs = $state(initialFollow);
  let seen = 0;

  async function toBottom(smooth: boolean) {
    await tick();
    box?.scrollTo({ top: box.scrollHeight, behavior: smooth ? "smooth" : "auto" });
  }

  $effect(() => {
    const added = lines.length - seen;
    seen = lines.length;
    if (added <= 0) return;
    if (fs.follow) toBottom(true);
    else fs = onLines(fs, added);
  });

  function handleScroll() {
    fs = onScroll(fs, isNearBottom(box.scrollTop, box.clientHeight, box.scrollHeight));
  }

  function live() {
    fs = returnToLive();
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
  <div class="tools" role="toolbar" aria-label="Transcript view">
    <button title="Smaller text" aria-label="Smaller text" disabled={prefs.lineText === 0} onclick={() => bump(-1)}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5.2" /><path d="M12.5 12.5L17 17M6 8.5h5" /></svg>
    </button>
    <button title="Larger text" aria-label="Larger text" disabled={prefs.lineText === LINE_TEXT.length - 1} onclick={() => bump(1)}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5.2" /><path d="M12.5 12.5L17 17M6 8.5h5M8.5 6v5" /></svg>
    </button>
    <button title={prefs.liveLine ? "Hide live line" : "Show live line"} aria-label={prefs.liveLine ? "Hide live line" : "Show live line"} aria-pressed={prefs.liveLine} class:on={prefs.liveLine} onclick={() => setPref("liveLine", !prefs.liveLine)}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2.5" y="4.5" width="15" height="11" rx="3" /><path d="M8.3 8.8a1.8 1.8 0 100 2.4M13.3 8.8a1.8 1.8 0 100 2.4" /></svg>
    </button>
  </div>
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
    aria-keyshortcuts="End"
    tabindex="0"
    style:--line-size="{size}px"
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
      {#if !liveOn}
      <div class="empty">
        <svg viewBox="0 0 48 32" aria-hidden="true"><rect x="4" y="12" width="5" height="8" rx="2.5" /><rect x="13" y="6" width="5" height="20" rx="2.5" /><rect x="22" y="1" width="5" height="30" rx="2.5" /><rect x="31" y="8" width="5" height="16" rx="2.5" /><rect x="40" y="12" width="5" height="8" rx="2.5" /></svg>
        {#if needsModel}
          <h2>One quick step first</h2>
          <p>SprkNotes needs a transcription model to turn speech into text.</p>
          {@render empty?.()}
        {:else if status === "ready"}
          <h2>Ready when you are</h2>
          <p>Press Start and your meeting appears here as it happens.</p>
        {:else}
          <h2>Listening</h2>
          <p>The first words will show up in a moment.</p>
        {/if}
      </div>
      {/if}
    {/each}
    {#if liveOn && partial}
      <p class="line live" class:me={partial.speaker === "Me"} class:others={partial.speaker !== "Me"}>
        <time>live</time><strong>{partial.speaker}</strong>
        <span class="text">{#each liveWords as w, i (i)}<span class="w" class:tent={i >= partial.stable} class:fixed={fixed.has(i)}>{w + " "}</span>{/each}<i class="caret"></i></span>
      </p>
    {/if}
  </div>
  {#if !fs.follow}
    <button class="pill" onclick={live}>
      Return to live{fs.unseen > 0 ? ` (${fs.unseen} new)` : ""}
    </button>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .tools {
    display: flex;
    justify-content: flex-end;
    gap: 0.25rem;
    padding: 0.35rem 0.6rem 0;
  }
  .tools button {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .tools button:hover:not(:disabled) { background: var(--surface-2); color: var(--text); }
  .tools button:disabled { opacity: 0.4; cursor: default; }
  .tools button.on { color: var(--accent); }
  .tools svg { width: 1.15rem; height: 1.15rem; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .pane {
    flex: 1;
    overflow-y: auto;
    overflow-anchor: auto;
    padding: 0.25rem 0.5rem 0.75rem;
  }
  .line {
    display: grid;
    grid-template-columns: 4.6rem 4.8rem 1fr;
    align-items: baseline;
    gap: 0.25rem;
    margin: 0;
    padding: 0.45rem 0.9rem;
    border-radius: 8px;
    font-size: var(--line-size, 15px);
    line-height: 1.5;
    /* Long sessions: off-screen lines skip layout and paint. */
    content-visibility: auto;
    contain-intrinsic-size: auto 2.4rem;
  }
  .line:hover { background: var(--surface-2); }
  @media (max-width: 30rem) {
    .line { grid-template-columns: auto 1fr; }
    .text { grid-column: 1 / -1; }
    .event .text { grid-column: 1 / -1; }
  }
  time { color: var(--muted); font-variant-numeric: tabular-nums; font-size: 0.8em; }
  strong { font-size: 0.85em; font-weight: 650; }
  .me strong { color: var(--me); }
  .others strong { color: var(--others); }
  .event { color: var(--muted); font-size: 0.9em; }
  .event .text { grid-column: 2 / -1; }
  .live { content-visibility: visible; background: color-mix(in srgb, var(--accent) 7%, transparent); }
  .live time { color: var(--error-text); font-weight: 600; }
  .w { animation: wordin 0.25s ease-out; }
  .w.tent { color: var(--muted); text-decoration: underline dotted; text-decoration-color: var(--muted); text-underline-offset: 0.2em; }
  .w.fixed { background: color-mix(in srgb, var(--accent) 22%, transparent); border-radius: 4px; transition: background 1.2s; }
  .caret { display: inline-block; width: 2px; height: 1.05em; margin-left: 1px; vertical-align: text-bottom; background: var(--live); animation: blink 1s steps(2, start) infinite; }
  @keyframes wordin { from { opacity: 0; } }
  @keyframes blink { to { visibility: hidden; } }
  @media (prefers-reduced-motion: reduce) {
    .w, .caret { animation: none; }
  }
  @media (forced-colors: active) {
    .w.tent { text-decoration: underline dotted; }
    .w.fixed { outline: 1px solid Highlight; }
    .caret { background: CanvasText; }
  }
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

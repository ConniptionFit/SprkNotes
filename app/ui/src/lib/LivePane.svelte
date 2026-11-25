<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import type { Caption, Line, Status } from "./types";
  import { prefs, setPref, LINE_TEXT, DELAY_MS } from "./prefs.svelte";
  import { isNearBottom, initialFollow, onScroll, onLines, returnToLive, UP_KEYS, USER_INPUT_WINDOW_MS } from "./follow";
  import { Typist, type Row, type Shown } from "./live";

  let {
    lines,
    status = "ready",
    needsModel = false,
    empty,
    partials = [],
    rows = [],
  }: { lines: Line[]; status?: Status; needsModel?: boolean; empty?: Snippet; partials?: Caption[]; rows?: Row[] } = $props();

  // Live rows: the sentence each speaker is saying, typed out at the bottom of the transcript, one
  // row per speaker so two people talking at once never take turns in one row. Locked sentences
  // (`rows`) sit above them; only the last word or two are soft (ADR-008). The main window shows
  // both speakers: it is the transcript, and Me waiting for the commit delay felt broken.
  const liveOn = $derived(prefs.liveLine && status !== "ready");
  const size = $derived(LINE_TEXT[prefs.lineText]);
  const bump = (d: number) => setPref("lineText", Math.min(LINE_TEXT.length - 1, Math.max(0, prefs.lineText + d)));

  type LiveRow = { speaker: string; words: Shown[] };
  let liveRows = $state<LiveRow[]>([]);
  const typists = new Map<string, Typist>();
  let lastFrame = 0;
  const reduce = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  function frame(now: number) {
    const dt = lastFrame ? Math.min(100, now - lastFrame) : 16;
    lastFrame = now;
    const current = liveOn ? partials : [];
    let changed = false;
    for (const speaker of typists.keys()) {
      if (!current.some((c) => c.speaker === speaker)) {
        typists.delete(speaker);
        changed = true;
      }
    }
    const next: LiveRow[] = [];
    for (const c of current) {
      let t = typists.get(c.speaker);
      if (!t) {
        t = new Typist();
        typists.set(c.speaker, t);
        changed = true;
      }
      if (t.step(c, now, dt, DELAY_MS[prefs.captionDelay], reduce)) changed = true;
      if (t.shown.length > 0 || t.recent) next.push({ speaker: c.speaker, words: t.shown });
    }
    if (changed || next.length !== liveRows.length || next.some((r, i) => r.speaker !== liveRows[i].speaker)) liveRows = next;
  }

  onMount(() => {
    let raf = 0;
    const loop = (t: number) => {
      frame(t);
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    // Whatever changes the height of the transcript keeps the end in view while following.
    const ro = new ResizeObserver(() => {
      if (!fs.follow) {
        lastHeight = box.scrollHeight;
        lastClient = box.clientHeight;
        return;
      }
      const before = lastHeight;
      const resized = box.clientHeight !== lastClient;
      lastClient = box.clientHeight;
      pin();
      glide(box.scrollHeight - before, resized);
    });
    ro.observe(stack);
    ro.observe(box);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  });

  let box: HTMLDivElement;
  let stack: HTMLDivElement;
  let fs = $state(initialFollow);
  let seen = 0;
  // The reader's own input, so a scroll the app caused is never mistaken for it.
  let lastInput = 0;
  let pressed = false;
  let lastTop = 0;
  let lastHeight = 0;

  let lastClient = 0;

  /**
   * The end is pinned instantly (an animation would race against text that keeps growing), so a
   * new line would snap the content up. Slide it instead: start where it was and ease to rest.
   * Slides add up, so several in a row stay smooth.
   */
  function glide(delta: number, resized: boolean) {
    if (reduce || resized || !stack || delta === 0 || Math.abs(delta) > 300 || !stack.animate) return;
    stack.animate([{ transform: `translateY(${delta}px)` }, { transform: "translateY(0)" }], {
      duration: 220,
      easing: "cubic-bezier(0.22, 0.61, 0.36, 1)",
      composite: "add",
    });
  }

  /** Jump to the end. Instant: an animation would race against text that keeps growing. */
  function pin() {
    if (!box) return;
    box.scrollTop = box.scrollHeight;
    lastTop = box.scrollTop;
    lastHeight = box.scrollHeight;
  }

  $effect(() => {
    const added = lines.length - seen;
    seen = lines.length;
    if (added <= 0) return;
    if (fs.follow) pin();
    else fs = onLines(fs, added);
  });

  const touch = () => (lastInput = performance.now());

  function handleScroll() {
    const byReader = pressed || performance.now() - lastInput < USER_INPUT_WINDOW_MS;
    // Content getting shorter makes the browser move the view up by itself; that is not the reader.
    const up = box.scrollTop < lastTop - 1 && box.scrollHeight >= lastHeight - 1;
    lastTop = box.scrollTop;
    lastHeight = box.scrollHeight;
    if (byReader && up) fs = { ...fs, follow: false };
    else fs = onScroll(fs, isNearBottom(box.scrollTop, box.clientHeight, box.scrollHeight), byReader);
  }

  function live() {
    fs = returnToLive();
    pin();
  }

  function key(e: KeyboardEvent) {
    if (e.key === "End") {
      e.preventDefault();
      live();
    } else if (UP_KEYS.includes(e.key)) touch();
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
    onwheel={(e) => e.deltaY < 0 && touch()}
    ontouchmove={touch}
    onpointerdown={() => (pressed = true)}
    onpointerup={() => (pressed = false)}
    onpointercancel={() => (pressed = false)}
    role="log"
    aria-live="off"
    aria-label="Live transcript"
    aria-keyshortcuts="End"
    tabindex="0"
    style:--line-size="{size}px"
  >
    <div class="stack" bind:this={stack}>
    {#each lines as l (l.id)}
      {#if l.kind === "statement"}
        <p class="line" class:me={l.speaker === "Me"} class:others={l.speaker !== "Me"}>
          <time>{l.t}</time><strong>{l.speaker}</strong><span class="text">{l.text}</span>
        </p>
      {:else}
        <p class="line event"><time>{l.t}</time><span class="text">{l.text}</span></p>
      {/if}
    {:else}
      {#if liveRows.length === 0 && rows.length === 0}
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
    {#each rows as r (r.uid)}
      <p class="line row" class:me={r.speaker === "Me"} class:others={r.speaker !== "Me"}>
        <time></time><strong>{r.speaker}</strong><span class="text">{r.text}</span>
      </p>
    {/each}
    {#each liveRows as lr (lr.speaker)}
      <p class="line live" class:me={lr.speaker === "Me"} class:others={lr.speaker !== "Me"}>
        <time>live</time><strong>{lr.speaker}</strong>
        <span class="text">{#each lr.words as w, i (i + ":" + w.w + w.hide)}<span class="w" class:soft={w.soft}>{w.w}<span class="rest">{w.hide}</span>{" "}</span>{/each}<i class="caret"></i></span>
      </p>
    {/each}
    </div>
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
    /* The browser's own anchoring fights the app's pinning; the app keeps the end in view. */
    overflow-anchor: none;
    padding: 0.25rem 0.5rem 0.75rem;
  }
  /* Bottom anchored: short transcripts sit at the bottom edge, new rows push older ones up. */
  .stack {
    min-height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
  }
  .stack > .line { flex: none; }
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
  .live { min-height: 1.5em; }
  /* Words ease between soft and settled, and fade in, so nothing pops. */
  .w { transition: opacity 0.3s ease; animation: wordin 0.2s ease-out; }
  .soft { opacity: 0.6; }
  .rest { visibility: hidden; }
  @keyframes wordin { from { opacity: 0.25; } }
  .caret { display: inline-block; width: 2px; height: 1.05em; margin-left: 1px; vertical-align: text-bottom; background: var(--live); animation: blink 1s steps(2, start) infinite; }
  .row { animation: rowin 0.15s ease-out; }
  @keyframes rowin { from { opacity: 0.4; } }
  @keyframes blink { to { visibility: hidden; } }
  @media (prefers-reduced-motion: reduce) {
    .row, .caret, .w { animation: none; transition: none; }
  }
  @media (forced-colors: active) {
    .soft { opacity: 1; text-decoration: underline dotted; }
    .rest { visibility: hidden; }
    .caret { background: CanvasText; }
  }
  .empty {
    flex: 1;
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

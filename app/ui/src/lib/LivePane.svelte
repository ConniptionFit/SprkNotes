<script lang="ts">
  import { onMount, tick, untrack, type Snippet } from "svelte";
  import type { Caption, Line, Status } from "./types";
  import { prefs, setPref, LINE_TEXT, DELAY_MS } from "./prefs.svelte";
  import { isNearBottom, initialFollow, onScroll, onLines, returnToLive, UP_KEYS, USER_INPUT_WINDOW_MS } from "./follow";
  import { Typist, type Row, type Shown } from "./live";
  import { lastSpeaker, turnStarts } from "./turns";
  import { findKeys, isFindKey, splitHits, step } from "./find";
  import { collectMarks, lineMarkName, markLabel, type MarkItem } from "./marks";
  import MarkBar from "./MarkBar.svelte";

  let {
    lines,
    status = "ready",
    needsModel = false,
    empty,
    partials = [],
    rows = [],
    onmark,
    onnote,
    markDisabled = false,
  }: {
    lines: Line[];
    status?: Status;
    needsModel?: boolean;
    empty?: Snippet;
    partials?: Caption[];
    rows?: Row[];
    /** Marks and notes are taken while a session runs (transcribing or paused). */
    onmark?: () => Promise<void> | void;
    onnote?: (text: string) => Promise<void> | void;
    markDisabled?: boolean;
  } = $props();

  // The transcript reads as a conversation (SPEC 8.1): each line is a bubble, Others on the left
  // and Me on the right, tinted in the speaker's colour. A speaker's name shows once per turn.
  // Live rows: the sentence each speaker is saying, typed out at the bottom, one bubble per speaker
  // so two people talking at once never take turns in one bubble. Locked sentences (`rows`) sit
  // above them; only the last word or two are soft (ADR-008).
  const liveOn = $derived(prefs.liveLine && status !== "ready");
  const marking = $derived(status !== "ready" && !!onmark && !!onnote);
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

  // Marks, with the names their notes give them; naming notes show on their mark, not as notes.
  const marks = $derived(collectMarks(lines));
  const visible = $derived(lines.filter((l) => !marks.hidden.has(l.id)));
  const speakerOf = (l: Line) => (l.kind === "statement" ? l.speaker : null);
  const lineStarts = $derived(turnStarts(visible.map(speakerOf)));
  const afterLines = $derived(lastSpeaker(visible.map(speakerOf)));
  const rowStarts = $derived(turnStarts(rows.map((r) => r.speaker), afterLines));
  const afterRows = $derived(lastSpeaker(rows.map((r) => r.speaker), afterLines));
  const liveStarts = $derived(turnStarts(liveRows.map((r) => r.speaker), afterRows));
  const side = (speaker: string) => (speaker === "Me" ? "me" : "others");

  // Find (Ctrl+F): searches what the window shows, newest match first.
  let findOpen = $state(false);
  let query = $state("");
  let hitAt = $state(-1);
  let findInput = $state<HTMLInputElement>();
  const entries = $derived([
    ...visible.map((l) => ({
      key: `l${l.id}`,
      text: l.kind === "mark" ? (marks.byLine.get(l.id)?.name ?? "") : l.kind === "statement" || l.kind === "note" || l.kind === "event" ? l.text : "",
    })),
    ...rows.map((r) => ({ key: `r${r.uid}`, text: r.text })),
  ]);
  const hits = $derived(findOpen ? findKeys(entries, query) : []);
  const hitSet = $derived(new Set(hits));
  const currentHit = $derived(hitAt >= 0 ? hits[hitAt] : undefined);

  async function openFind() {
    findOpen = true;
    await tick();
    findInput?.focus();
    findInput?.select();
  }
  function closeFind() {
    findOpen = false;
    query = "";
    hitAt = -1;
    box?.focus();
  }
  // A new search starts at the newest match, the one nearest the live end.
  $effect(() => {
    void query;
    untrack(() => {
      hitAt = hits.length > 0 ? hits.length - 1 : -1;
      if (hitAt >= 0) go(hits[hitAt]);
    });
  });
  function nextHit(older: boolean) {
    hitAt = step(hits.length, hitAt, older);
    if (hitAt >= 0) go(hits[hitAt]);
  }
  function findKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeFind();
    } else if (e.key === "Enter") {
      e.preventDefault();
      nextHit(!e.shiftKey);
    }
  }

  // Marks list: every mark in time order, to jump to and to name.
  let marksOpen = $state(false);
  let editing = $state<number | null>(null);
  let draft = $state("");
  let flash = $state<string | null>(null);
  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    if (marks.items.length === 0) marksOpen = false;
  });

  function startNaming(m: MarkItem) {
    editing = m.id;
    draft = m.name;
  }
  async function saveName(m: MarkItem) {
    // Enter saves and the box then loses focus; only the first of the two counts.
    if (editing !== m.id) return;
    const name = draft.trim();
    editing = null;
    if (!name || name === m.name || !onnote) return;
    try {
      await onnote(markLabel(m.t, name));
      bar?.tell("Mark named");
    } catch {
      bar?.tell("The name could not be saved.", true);
    }
  }
  function nameKey(e: KeyboardEvent, m: MarkItem) {
    if (e.key === "Enter") {
      e.preventDefault();
      saveName(m);
    } else if (e.key === "Escape") {
      e.preventDefault();
      editing = null;
    }
  }

  /** Bring one line into view (a find match or a mark) and stop following live. */
  function go(key: string) {
    if (!box) return;
    const el = box.querySelector<HTMLElement>(`[data-key="${key}"]`);
    if (!el) return;
    fs = { follow: false, unseen: fs.unseen };
    el.scrollIntoView({ block: "center", behavior: reduce ? "auto" : "smooth" });
    flash = key;
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (flash = null), 1600);
  }

  // Line actions on hover: copy, mark this line, add a note about it.
  let hovered = $state<number | null>(null);
  let copied = $state<number | null>(null);
  let bar = $state<ReturnType<typeof MarkBar>>();

  async function copy(l: Line & { kind: "statement" }) {
    try {
      await navigator.clipboard.writeText(l.text);
      copied = l.id;
      setTimeout(() => copied === l.id && (copied = null), 1500);
      bar?.tell("Copied");
    } catch {
      bar?.tell("The line could not be copied.", true);
    }
  }
  async function markLine(l: Line & { kind: "statement" }) {
    if (!onnote) return;
    try {
      await onnote(markLabel(l.t, lineMarkName(l.speaker, l.text)));
      bar?.tell("Line marked");
    } catch {
      bar?.tell("The line could not be marked.", true);
    }
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
      clearTimeout(flashTimer);
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

  function windowKey(e: KeyboardEvent) {
    if (!isFindKey(e)) return;
    e.preventDefault();
    openFind();
  }
</script>

<svelte:window onkeydown={windowKey} />

{#snippet text(key: string, body: string)}
  {#if findOpen && hitSet.has(key)}
    {#each splitHits(body, query) as p, i (i)}{#if p.hit}<mark class:current={currentHit === key}>{p.text}</mark>{:else}{p.text}{/if}{/each}
  {:else}
    {body}
  {/if}
{/snippet}

{#snippet bookmark()}
  <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 3.5h9v13l-4.5-3.2-4.5 3.2z" /></svg>
{/snippet}

{#snippet nameBox(m: MarkItem)}
  <!-- svelte-ignore a11y_autofocus -->
  <input
    class="namebox"
    bind:value={draft}
    maxlength="120"
    placeholder="Name this mark"
    aria-label="Mark name"
    autofocus
    onkeydown={(e) => nameKey(e, m)}
    onblur={() => saveName(m)}
  />
{/snippet}

<div class="wrap" style:--line-size="{size}px">
  <div class="tools" role="toolbar" aria-label="Transcript view">
    <button class="tool" title="Find in transcript (Ctrl+F)" aria-label="Find in transcript" aria-keyshortcuts="Control+F" aria-pressed={findOpen} class:on={findOpen} onclick={() => (findOpen ? closeFind() : openFind())}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5.2" /><path d="M12.5 12.5L17 17" /></svg>
      <span>Find</span>
    </button>
    <div class="size" role="group" aria-label="Text size">
      <button title="Smaller text" aria-label="Smaller text" disabled={prefs.lineText === 0} onclick={() => bump(-1)}>A<span class="minus">−</span></button>
      <button title="Larger text" aria-label="Larger text" disabled={prefs.lineText === LINE_TEXT.length - 1} onclick={() => bump(1)}>A<span class="plus">+</span></button>
    </div>
    <button class="tool" title={prefs.liveLine ? "Hide live line" : "Show live line"} aria-pressed={prefs.liveLine} class:on={prefs.liveLine} onclick={() => setPref("liveLine", !prefs.liveLine)}>
      <span class="dot" aria-hidden="true"></span>
      <span>Live line</span>
    </button>
    {#if marks.items.length > 0}
      <button class="tool" title="Marks in this session" aria-pressed={marksOpen} class:on={marksOpen} onclick={() => (marksOpen = !marksOpen)}>
        {@render bookmark()}
        <span>Marks</span>
        <span class="count">{marks.items.length}</span>
      </button>
    {/if}
  </div>
  {#if findOpen}
    <div class="find" role="search">
      <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5.2" /><path d="M12.5 12.5L17 17" /></svg>
      <input
        bind:this={findInput}
        bind:value={query}
        onkeydown={findKey}
        placeholder="Find in transcript"
        aria-label="Find in transcript"
      />
      <span class="hits" role="status">{query.trim() ? (hits.length ? `${hitAt + 1} of ${hits.length}` : "No matches") : ""}</span>
      <button title="Older match (Enter)" aria-label="Older match" disabled={hits.length === 0} onclick={() => nextHit(true)}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 12l4.5-4.5 4.5 4.5" /></svg>
      </button>
      <button title="Newer match (Shift+Enter)" aria-label="Newer match" disabled={hits.length === 0} onclick={() => nextHit(false)}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 8l4.5 4.5 4.5-4.5" /></svg>
      </button>
      <button title="Close (Esc)" aria-label="Close find" onclick={closeFind}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 5.5l9 9M14.5 5.5l-9 9" /></svg>
      </button>
    </div>
  {/if}
  <div class="middle">
  <!-- A scrollable log must be focusable so the keyboard can scroll it and End can return to live. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="pane"
    class:always={prefs.messageTimes === "always"}
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
  >
    <div class="stack" bind:this={stack}>
    {#each visible as l, i (l.id)}
      {#if l.kind === "statement"}
        {@const m = marks.byLine.get(l.id)}
        <!-- Hover shows the line's actions; the same actions are in the bar and the toolbar for the keyboard. -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div
          class="msg {side(l.speaker)}"
          class:start={lineStarts[i]}
          class:flash={flash === `l${l.id}`}
          data-key="l{l.id}"
          role="group"
          aria-label="{l.speaker}, {l.t}"
          onpointerenter={() => (hovered = l.id)}
          onpointerleave={() => hovered === l.id && (hovered = null)}
        >
          {#if lineStarts[i]}<p class="who"><strong>{l.speaker}</strong></p>{/if}
          <div class="body">
            <p class="bubble" class:marked={!!m}>
              {#if m}<span class="flag" title={m.name || "Marked"}>{@render bookmark()}</span>{/if}
              {@render text(`l${l.id}`, l.text)}
            </p>
            <div class="side">
              {#if hovered === l.id}
                <span class="acts">
                  <button title={copied === l.id ? "Copied" : "Copy line"} aria-label="Copy line" onclick={() => copy(l)}>
                    {#if copied === l.id}
                      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4.5 10.5l3.5 3.5 7.5-8" /></svg>
                    {:else}
                      <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="6.5" y="6.5" width="9" height="10" rx="2" /><path d="M4.5 13V5.5a2 2 0 012-2h6" /></svg>
                    {/if}
                  </button>
                  {#if marking && !m}
                    <button title="Mark this line" aria-label="Mark this line" disabled={markDisabled} onclick={() => markLine(l)}>{@render bookmark()}</button>
                  {/if}
                  {#if marking}
                    <button title="Add a note about this line" aria-label="Add a note about this line" disabled={markDisabled} onclick={() => bar?.prefill(`About ${l.t}: `)}>
                      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 15.5V13l8.5-8.5 2.5 2.5-8.5 8.5zM11 6l2.5 2.5" /></svg>
                    </button>
                  {/if}
                </span>
              {/if}
              <time class="stamp">{l.t}</time>
            </div>
          </div>
        </div>
      {:else if l.kind === "mark"}
        {@const m = marks.byLine.get(l.id)}
        <div class="center" class:flash={flash === `l${l.id}`} data-key="l{l.id}">
          <p class="chip">
            {@render bookmark()}
            {#if m && editing === m.id}
              {@render nameBox(m)}
            {:else}
              <span class="label">{#if m?.name}{@render text(`l${l.id}`, m.name)}{:else}Mark{/if}</span>
              <time>{l.t}</time>
              {#if marking && m}
                <button class="pen" title={m.name ? "Rename this mark" : "Name this mark"} aria-label={m.name ? "Rename this mark" : "Name this mark"} onclick={() => startNaming(m)}>
                  <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 15.5V13l8.5-8.5 2.5 2.5-8.5 8.5zM11 6l2.5 2.5" /></svg>
                </button>
              {/if}
            {/if}
          </p>
        </div>
      {:else if l.kind === "note"}
        <div class="center" class:flash={flash === `l${l.id}`} data-key="l{l.id}">
          <div class="note">
            <p class="head"><strong>Note</strong><time>{l.t}</time></p>
            <p>{@render text(`l${l.id}`, l.text)}</p>
          </div>
        </div>
      {:else}
        <div class="event" class:flash={flash === `l${l.id}`} data-key="l{l.id}">
          <span>{@render text(`l${l.id}`, l.text)}</span><time>{l.t}</time>
        </div>
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
    {#each rows as r, i (r.uid)}
      <div class="msg row {side(r.speaker)}" class:start={rowStarts[i]} class:flash={flash === `r${r.uid}`} data-key="r{r.uid}">
        {#if rowStarts[i]}<p class="who"><strong>{r.speaker}</strong></p>{/if}
        <div class="body">
          <p class="bubble">{@render text(`r${r.uid}`, r.text)}</p>
          <div class="side"><time class="stamp">{r.t ?? ""}</time></div>
        </div>
      </div>
    {/each}
    {#each liveRows as lr, i (lr.speaker)}
      <div class="msg live {side(lr.speaker)}" class:start={liveStarts[i]}>
        <p class="who">{#if liveStarts[i]}<strong>{lr.speaker}</strong>{/if}<span class="speaking"><i aria-hidden="true"></i>speaking</span></p>
        <div class="body">
          <p class="bubble">{#each lr.words as w, j (j + ":" + w.w + w.hide)}<span class="w" class:soft={w.soft}>{w.w}<span class="rest">{w.hide}</span>{" "}</span>{/each}<i class="caret"></i></p>
        </div>
      </div>
    {/each}
    </div>
  </div>
  {#if marksOpen}
    <aside class="marks" aria-label="Marks">
      <p class="title">Marks <span>{marks.items.length}</span></p>
      <ul>
        {#each marks.items as m (m.id)}
          <li>
            {#if editing === m.id}
              {@render nameBox(m)}
            {:else}
              <button class="jump" onclick={() => go(`l${m.id}`)}>
                <time>{m.t}</time>
                <span class:unnamed={!m.name}>{m.name || "Unnamed mark"}</span>
              </button>
              {#if marking}
                <button class="pen" title={m.name ? "Rename this mark" : "Name this mark"} aria-label={m.name ? `Rename the mark at ${m.t}` : `Name the mark at ${m.t}`} onclick={() => startNaming(m)}>
                  <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 15.5V13l8.5-8.5 2.5 2.5-8.5 8.5zM11 6l2.5 2.5" /></svg>
                </button>
              {/if}
            {/if}
          </li>
        {/each}
      </ul>
    </aside>
  {/if}
  {#if !fs.follow}
    <button class="pill" onclick={live}>
      Return to live{fs.unseen > 0 ? ` (${fs.unseen} new)` : ""}
    </button>
  {/if}
  </div>
  {#if marking}
    <MarkBar bind:this={bar} disabled={markDisabled} onmark={() => onmark?.()} onnote={(t) => onnote?.(t)} />
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
  svg { fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }

  /* Toolbar: Find, text size, live line, marks. */
  .tools {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.35rem;
    padding: 0.4rem 0.6rem 0.35rem;
    border-bottom: 1px solid var(--border);
  }
  .tools button {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    height: 2rem;
    padding: 0 0.7rem;
    border: 1px solid transparent;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
  }
  .tools button:hover:not(:disabled) { background: var(--surface-2); color: var(--text); }
  .tools button:disabled { opacity: 0.4; cursor: default; }
  .tools .on { color: var(--accent); background: var(--accent-soft); }
  .tools svg { width: 1rem; height: 1rem; }
  .size { display: inline-flex; border: 1px solid var(--border); border-radius: 999px; overflow: hidden; }
  .size button { border-radius: 0; padding: 0 0.65rem; font-size: 0.95rem; color: var(--text); }
  .size button + button { border-left: 1px solid var(--border); }
  .size .minus, .size .plus { font-size: 0.75em; margin-left: 1px; }
  .dot { width: 0.5rem; height: 0.5rem; border-radius: 50%; background: currentColor; opacity: 0.5; }
  .on .dot { background: var(--live); opacity: 1; }
  .count { min-width: 1.2rem; padding: 0 0.3rem; border-radius: 999px; background: var(--accent); color: var(--on-accent); font-size: 0.72rem; line-height: 1.2rem; text-align: center; }

  /* Find bar. */
  .find {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.4rem 0.6rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
  }
  .find > svg { width: 1rem; height: 1rem; color: var(--muted); margin-left: 0.25rem; }
  .find input {
    flex: 1;
    min-width: 6rem;
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: 0.92rem;
  }
  .find input:focus-visible { outline: 2px solid var(--accent); outline-offset: 0; }
  .hits { min-width: 5rem; color: var(--muted); font-size: 0.85rem; text-align: right; font-variant-numeric: tabular-nums; }
  .find button { display: grid; place-items: center; width: 1.9rem; height: 1.9rem; border: 0; border-radius: 8px; background: transparent; color: var(--muted); cursor: pointer; }
  .find button:hover:not(:disabled) { background: var(--surface); color: var(--text); }
  .find button:disabled { opacity: 0.4; cursor: default; }
  .find button svg { width: 1rem; height: 1rem; }
  mark { background: var(--warn-soft); color: inherit; border-radius: 3px; box-shadow: 0 0 0 1px var(--warn-soft); }
  mark.current { background: var(--warn); color: var(--surface); box-shadow: 0 0 0 1px var(--warn); }

  .middle { position: relative; flex: 1; min-height: 0; display: flex; }
  .pane {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    /* The browser's own anchoring fights the app's pinning; the app keeps the end in view. */
    overflow-anchor: none;
    padding: 0.5rem 1rem 0.9rem;
  }
  /* Bottom anchored: short transcripts sit at the bottom edge, new rows push older ones up.
     The column is capped so lines stay an easy reading length on wide windows. */
  .stack {
    min-height: 100%;
    max-width: 52rem;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
  }
  .stack > * { flex: none; }

  /* Speech: bubbles, Others on the left and Me on the right, in the speaker's colour. */
  .msg {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    margin-top: 0.2rem;
    /* Long sessions: off-screen lines skip layout and paint. */
    content-visibility: auto;
    contain-intrinsic-size: auto 2.6rem;
  }
  .msg.me { align-items: flex-end; }
  .msg.start { margin-top: 0.85rem; }
  .who { display: flex; align-items: center; gap: 0.5rem; margin: 0 0.6rem 0.2rem; font-size: 0.82rem; }
  .who strong { font-weight: 650; }
  .me .who strong { color: var(--me); }
  .others .who strong { color: var(--others); }
  .body { display: flex; align-items: flex-end; gap: 0.45rem; width: 100%; }
  .me .body { flex-direction: row-reverse; }
  .bubble {
    position: relative;
    max-width: min(42em, calc(100% - 9.5rem));
    margin: 0;
    padding: 0.45em 0.85em;
    border-radius: 1.05em;
    font-size: var(--line-size, 15px);
    line-height: 1.5;
    overflow-wrap: anywhere;
    color: var(--text);
  }
  .others .bubble { background: var(--others-soft); border-top-left-radius: 0.35em; }
  .me .bubble { background: var(--me-soft); border-top-right-radius: 0.35em; }
  .others:not(.start) .bubble { border-top-left-radius: 1.05em; border-bottom-left-radius: 0.35em; }
  .me:not(.start) .bubble { border-top-right-radius: 1.05em; border-bottom-right-radius: 0.35em; }
  .bubble.marked { box-shadow: inset 0 0 0 1.5px var(--accent); }
  .flag { display: inline-flex; vertical-align: -0.12em; margin-right: 0.3em; color: var(--accent); }
  .flag svg { width: 0.95em; height: 0.95em; }
  .side { display: flex; align-items: center; gap: 0.35rem; flex: none; min-height: 1.6rem; }
  .me .side { flex-direction: row-reverse; }
  .stamp { color: var(--muted); font-size: 0.75rem; font-variant-numeric: tabular-nums; opacity: 0; transition: opacity 120ms; }
  .always .stamp, .msg:hover .stamp, .msg:focus-within .stamp { opacity: 1; }
  .acts { display: inline-flex; gap: 0.15rem; padding: 0.1rem; border: 1px solid var(--border); border-radius: 999px; background: var(--surface); box-shadow: var(--shadow); }
  .acts button { display: grid; place-items: center; width: 1.6rem; height: 1.6rem; border: 0; border-radius: 50%; background: transparent; color: var(--muted); cursor: pointer; }
  .acts button:hover:not(:disabled) { background: var(--surface-2); color: var(--accent); }
  .acts button:disabled { opacity: 0.4; cursor: default; }
  .acts svg { width: 0.95rem; height: 0.95rem; }
  .row { animation: rowin 0.15s ease-out; }
  @keyframes rowin { from { opacity: 0.4; } }

  /* The live bubble: dashed in the speaker's colour while the words are still coming. */
  .live { content-visibility: visible; }
  .live .bubble { min-height: 1.5em; background: transparent; border: 1.5px dashed var(--others); }
  .me.live .bubble { border-color: var(--me); }
  .speaking { display: inline-flex; align-items: center; gap: 0.3rem; color: var(--muted); font-size: 0.75rem; font-weight: 600; }
  .speaking i { width: 0.45rem; height: 0.45rem; border-radius: 50%; background: var(--others); animation: pulse 1.2s ease-in-out infinite; }
  .me .speaking i { background: var(--me); }
  @keyframes pulse { 50% { opacity: 0.35; } }
  /* Words ease between soft and settled, and fade in, so nothing pops. */
  .w { transition: opacity 0.3s ease; animation: wordin 0.2s ease-out; }
  .soft { opacity: 0.6; }
  .rest { visibility: hidden; }
  @keyframes wordin { from { opacity: 0.25; } }
  .caret { display: inline-block; width: 2px; height: 1.05em; margin-left: 1px; vertical-align: text-bottom; background: var(--others); animation: blink 1s steps(2, start) infinite; }
  .me .caret { background: var(--me); }
  @keyframes blink { to { visibility: hidden; } }

  /* Marks, notes and events sit in the middle, apart from the conversation (SPEC 5.1). */
  .center { display: flex; justify-content: center; margin: 0.85rem 0 0.2rem; content-visibility: auto; contain-intrinsic-size: auto 2.4rem; }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    max-width: 100%;
    margin: 0;
    padding: 0.3rem 0.5rem 0.3rem 0.75rem;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent);
    font-size: 0.85rem;
    font-weight: 650;
  }
  .chip > svg { width: 0.95rem; height: 0.95rem; flex: none; }
  .chip .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chip time, .note time, .event time { color: var(--muted); font-weight: 500; font-size: 0.75rem; font-variant-numeric: tabular-nums; }
  .pen { display: grid; place-items: center; width: 1.6rem; height: 1.6rem; flex: none; border: 0; border-radius: 50%; background: transparent; color: var(--muted); cursor: pointer; }
  .pen:hover { background: var(--surface); color: var(--accent); }
  .pen svg { width: 0.9rem; height: 0.9rem; }
  .namebox { min-width: 10rem; padding: 0.2rem 0.5rem; border: 1px solid var(--accent); border-radius: 6px; background: var(--surface); color: var(--text); font: inherit; font-size: 0.85rem; font-weight: 500; }
  .note {
    max-width: min(38em, 100%);
    padding: 0.45rem 0.85rem;
    border: 1.5px dashed var(--accent);
    border-radius: 10px;
    background: var(--surface);
    font-size: calc(var(--line-size, 15px) * 0.93);
    line-height: 1.45;
  }
  .note p { margin: 0; overflow-wrap: anywhere; }
  .note .head { display: flex; align-items: baseline; gap: 0.5rem; margin-bottom: 0.1rem; font-size: 0.8rem; }
  .note strong { color: var(--accent); }
  .event { display: flex; align-items: center; justify-content: center; gap: 0.5rem; margin: 0.85rem 0 0.2rem; color: var(--muted); font-size: 0.85rem; content-visibility: auto; contain-intrinsic-size: auto 1.6rem; }
  .event::before, .event::after { content: ""; flex: 1; max-width: 6rem; border-top: 1px solid var(--border); }
  .flash .bubble, .flash .chip, .flash .note, .flash.event span { animation: flash 1.6s ease-out; }
  @keyframes flash { 0%, 40% { box-shadow: 0 0 0 3px var(--accent); } 100% { box-shadow: 0 0 0 3px transparent; } }

  /* Marks list. */
  .marks {
    width: 17rem;
    flex: none;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--border);
    background: var(--surface);
    overflow-y: auto;
  }
  @media (max-width: 45rem) {
    .marks { position: absolute; top: 0; right: 0; bottom: 0; z-index: 3; box-shadow: var(--shadow), -8px 0 24px rgb(0 0 0 / 0.12); }
  }
  .marks .title { margin: 0; padding: 0.7rem 0.9rem 0.4rem; font-weight: 650; font-size: 0.9rem; }
  .marks .title span { color: var(--muted); font-weight: 500; }
  .marks ul { list-style: none; margin: 0; padding: 0 0.4rem 0.6rem; }
  .marks li { display: flex; align-items: center; gap: 0.2rem; }
  .jump { flex: 1; min-width: 0; display: flex; align-items: baseline; gap: 0.6rem; padding: 0.45rem 0.5rem; border: 0; border-radius: 8px; background: transparent; color: var(--text); font: inherit; font-size: 0.9rem; text-align: left; cursor: pointer; }
  .jump:hover { background: var(--surface-2); }
  .jump time { color: var(--muted); font-size: 0.78rem; font-variant-numeric: tabular-nums; flex: none; }
  .jump span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .jump .unnamed { color: var(--muted); font-style: italic; }
  .marks .namebox { flex: 1; margin: 0.25rem 0.2rem; }

  @media (max-width: 30rem) {
    .pane { padding: 0.4rem 0.5rem 0.8rem; }
    .bubble { max-width: calc(100% - 5rem); }
  }
  @media (prefers-reduced-motion: reduce) {
    .row, .caret, .w, .speaking i, .flash .bubble, .flash .chip, .flash .note, .flash.event span { animation: none; transition: none; }
  }
  @media (forced-colors: active) {
    .bubble, .live .bubble { border: 1px solid CanvasText; }
    .soft { opacity: 1; text-decoration: underline dotted; }
    .rest { visibility: hidden; }
    .caret { background: CanvasText; }
    mark.current { outline: 2px solid Highlight; }
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
  .empty svg { width: 4.5rem; fill: var(--accent); stroke: none; opacity: 0.55; margin-bottom: 0.75rem; }
  .empty h2 { margin: 0; font-size: 1.15rem; color: var(--text); }
  .empty p { margin: 0; }
  .pill {
    position: absolute;
    left: 50%;
    bottom: 1rem;
    transform: translateX(-50%);
    z-index: 2;
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

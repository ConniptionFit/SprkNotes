<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./api";
  import type { Caption, Status } from "./types";
  import { prefs, setPref } from "./prefs.svelte";
  import Light from "./Light.svelte";
  import IconButton from "./IconButton.svelte";

  const MAX_LINES = 3;

  let api: Backend | undefined;
  let status = $state<Status>("ready");
  let seconds = $state(0);
  let expanded = $state(false);
  let frame: HTMLDivElement;
  let collapseTimer: ReturnType<typeof setTimeout> | undefined;
  let hovering = false;

  // Captions: the last few committed lines plus the line still being spoken.
  let finals = $state<{ id: number; speaker: string; text: string }[]>([]);
  let partial = $state<Caption | null>(null);

  let nextId = 0;
  // Words already on screen per speaker, so only new words fade in, one after another.
  let shownCount: { [k: string]: number } = {};
  const wordsOf = (c: Caption) => c.text.split(/\s+/).filter(Boolean);
  const delay = (c: Caption, i: number) => Math.max(0, i - (shownCount[c.speaker] ?? 0)) * 70;

  const wanted = (speaker: string) => prefs.captionsWho === "both" || speaker !== "Me";
  const shown = $derived(finals.filter((l) => wanted(l.speaker)).slice(-MAX_LINES));
  const livePartial = $derived(partial && wanted(partial.speaker) ? partial : null);

  const clock = $derived.by(() => {
    const h = Math.floor(seconds / 3600);
    const mm = String(Math.floor(seconds / 60) % 60).padStart(2, "0");
    const ss = String(seconds % 60).padStart(2, "0");
    return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
  });

  onMount(() => {
    const offs: Array<() => void> = [];
    backend.then(async (b) => {
      api = b;
      offs.push(
        await b.onStatus((s) => {
          if (s === "transcribing" && status === "ready") {
            seconds = 0; // a new session
            finals = [];
          }
          status = s;
          if (s === "ready") partial = null;
        }),
      );
      // Finished lines arrive as final captions (the committed wording, shown right away).
      offs.push(
        await b.onCaption((c) => {
          if (c.final) {
            finals = [...finals.slice(-(MAX_LINES * 3)), { id: nextId++, speaker: c.speaker, text: c.text }];
            if (partial?.speaker === c.speaker) partial = null;
            shownCount[c.speaker] = 0;
            return;
          }
          partial = c;
          // Remember how much was already shown, after this render has used the old count.
          queueMicrotask(() => (shownCount[c.speaker] = wordsOf(c).length));
        }),
      );
    });

    // Elapsed time counts only while transcribing.
    const tick = setInterval(() => {
      if (status === "transcribing") seconds += 1;
    }, 1000);

    // The window is sized to its content, so the orb stays put and the window grows around it.
    const ro = new ResizeObserver(() => {
      const r = frame.getBoundingClientRect();
      api?.miniResize(Math.ceil(r.width), Math.ceil(r.height));
    });
    ro.observe(frame);

    return () => {
      offs.forEach((f) => f());
      clearInterval(tick);
      ro.disconnect();
    };
  });

  // Tell the engine which caption sources are wanted, so a source nobody reads costs nothing.
  $effect(() => {
    api?.setCaptions(prefs.captionsOn, prefs.captionsWho === "both").catch(() => {});
  });

  function refresh() {
    clearTimeout(collapseTimer);
    if (hovering || frame?.matches(":focus-within")) expanded = true;
    // Linger briefly so a stray mouse move never makes the buttons jump away.
    else collapseTimer = setTimeout(() => (expanded = false), 450);
  }
  const hover = (on: boolean) => ((hovering = on), refresh());
</script>

<main>
  <h1 class="sr-only">SprkNotes overlay</h1>
  <!-- Hover and focus only reveal the buttons; every action is a real button inside. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="frame"
    bind:this={frame}
    role="group"
    aria-label="SprkNotes overlay"
    onmouseenter={() => hover(true)}
    onmouseleave={() => hover(false)}
    onfocusin={refresh}
    onfocusout={() => setTimeout(refresh)}
    onkeydown={(e) => e.key === "Escape" && ((document.activeElement as HTMLElement | null)?.blur(), (expanded = false))}
  >
    <div class="orb">
      <!-- Always shown, always in the same place: grip, light, clock, captions. -->
      <div class="grip" data-tauri-drag-region title="Drag to move">
        <svg viewBox="0 0 10 18" aria-hidden="true">
          <circle cx="3" cy="3" r="1.3" /><circle cx="7" cy="3" r="1.3" />
          <circle cx="3" cy="9" r="1.3" /><circle cx="7" cy="9" r="1.3" />
          <circle cx="3" cy="15" r="1.3" /><circle cx="7" cy="15" r="1.3" />
        </svg>
      </div>
      <Light {status} size={11} />
      <span class="clock" class:idle={status === "ready"}>{clock}</span>
      <IconButton
        label={prefs.captionsOn ? "Hide captions" : "Show captions"}
        pressed={prefs.captionsOn}
        onclick={() => setPref("captionsOn", !prefs.captionsOn)}
      >
        <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2.5" y="4.5" width="15" height="11" rx="3" /><path d="M8.3 8.8a1.8 1.8 0 100 2.4M13.3 8.8a1.8 1.8 0 100 2.4" /></svg>
      </IconButton>

      {#if expanded}
        <div class="actions">
          {#if status === "ready"}
            <IconButton label="Start" primary onclick={() => api?.start("")}>
              <svg viewBox="0 0 20 20" aria-hidden="true"><path class="solid" d="M6.5 4.5l9 5.5-9 5.5z" /></svg>
            </IconButton>
          {:else}
            {#if status === "paused"}
              <IconButton label="Resume" primary onclick={() => api?.resume()}>
                <svg viewBox="0 0 20 20" aria-hidden="true"><path class="solid" d="M6.5 4.5l9 5.5-9 5.5z" /></svg>
              </IconButton>
            {:else}
              <IconButton label="Pause" primary onclick={() => api?.pause()}>
                <svg viewBox="0 0 20 20" aria-hidden="true"><path class="solid" d="M6 4.5h2.8v11H6zM11.2 4.5H14v11h-2.8z" /></svg>
              </IconButton>
            {/if}
            <IconButton label="Stop" onclick={() => api?.end()}>
              <svg viewBox="0 0 20 20" aria-hidden="true"><rect class="solid" x="5.5" y="5.5" width="9" height="9" rx="2" /></svg>
            </IconButton>
          {/if}
          <IconButton label="Open SprkNotes" onclick={() => api?.openApp()}>
            <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M11.5 3.5h5v5M16.5 3.5l-7 7M8 5.5H4.5v10h10V12" /></svg>
          </IconButton>
        </div>
      {/if}
    </div>

    {#if prefs.captionsOn}
      <!-- A fixed-height bar so new words never make the window jump. -->
      <div class="captions" role="log" aria-label="Live captions" aria-live="off">
        {#each shown as l (l.id)}
          <p class="cl old">{#if prefs.captionsWho === "both"}<b class="chip" class:me={l.speaker === "Me"}>{l.speaker}</b>{/if}{l.text}</p>
        {/each}
        {#if livePartial}
          <p class="cl live">{#if prefs.captionsWho === "both"}<b class="chip" class:me={livePartial.speaker === "Me"}>{livePartial.speaker}</b>{/if}{#each wordsOf(livePartial) as w, i (i)}<span class="w" class:tent={i >= livePartial.stable} style="--d: {delay(livePartial, i)}ms">{w} </span>{/each}<i class="caret"></i></p>
        {:else if shown.length === 0}
          <p class="cl hint">Captions appear here as people speak.</p>
        {/if}
      </div>
    {/if}
  </div>
</main>

<style>
  .frame { display: inline-flex; flex-direction: column; align-items: flex-start; gap: 8px; padding: 6px; user-select: none; }
  .orb {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 42px;
    padding: 0 8px 0 0;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    box-shadow: var(--shadow);
    white-space: nowrap;
  }
  .grip { align-self: stretch; display: flex; align-items: center; padding: 0 0 0 14px; color: var(--muted); cursor: grab; }
  .grip:active { cursor: grabbing; }
  .grip svg { width: 0.55rem; height: 1rem; fill: currentColor; pointer-events: none; }
  .clock { font-variant-numeric: tabular-nums; font-weight: 600; font-size: 0.9rem; min-width: 3.1rem; }
  .clock.idle { color: var(--muted); font-weight: 500; }
  .actions { display: flex; gap: 2px; align-items: center; padding-left: 6px; margin-left: 2px; border-left: 1px solid var(--border); }
  .captions {
    box-sizing: border-box;
    width: 420px;
    height: 7.4rem;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 0.15rem;
    overflow: hidden;
    padding: 0.75rem 1rem;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: color-mix(in srgb, var(--surface) 94%, transparent);
    box-shadow: var(--shadow);
    font-size: 1.05rem;
  }
  .cl { margin: 0; line-height: 1.4; }
  .cl.old { color: var(--muted); }
  .cl.live { color: var(--text); }
  .w { animation: wordin 160ms ease-out both; animation-delay: var(--d, 0ms); }
  .w.tent { opacity: 0.72; }
  @keyframes wordin { from { opacity: 0; transform: translateY(2px); } }
  @media (prefers-reduced-motion: reduce) { .w { animation: none; } }
  .cl.hint { color: var(--muted); font-size: 0.9rem; }
  .chip { display: inline-block; margin-right: 0.5rem; padding: 0 0.5rem; border-radius: 999px; font-size: 0.7rem; font-weight: 700; background: var(--others-soft); color: var(--others); }
  .chip.me { background: var(--me-soft); color: var(--me); }
  .caret { display: inline-block; width: 2px; height: 1em; margin-left: 3px; vertical-align: -2px; background: var(--accent); animation: blink 1s steps(2) infinite; }
  @keyframes blink { 50% { opacity: 0; } }
  @media (forced-colors: active) { .orb, .captions { border: 1px solid CanvasText; } }
</style>

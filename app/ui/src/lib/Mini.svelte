<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./api";
  import type { Caption, Status } from "./types";
  import { prefs, setPref, captionFlags, CAPTION_TEXT, CAPTION_LIMITS } from "./prefs.svelte";
  import Light from "./Light.svelte";
  import IconButton from "./IconButton.svelte";

  // Plenty of history for a tall drawer; whatever does not fit is clipped at the top.
  const MAX_LINES = 12;

  let api: Backend | undefined;
  let status = $state<Status>("ready");
  let seconds = $state(0);
  let expanded = $state(false);
  let frame: HTMLDivElement;
  let collapseTimer: ReturnType<typeof setTimeout> | undefined;
  let hovering = false;
  /** The caption drawer sits above the pill when the pill is low on the screen. */
  let flipped = $state(false);
  /** Live size while the handle is dragged; saved on release so storage is written once. */
  let drag = $state<{ w: number; h: number } | null>(null);
  const drawerW = $derived(drag?.w ?? prefs.captionW);
  const drawerH = $derived(drag?.h ?? prefs.captionH);
  const textStep = $derived(prefs.captionText);

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
      offs.push(await b.onMiniFlip((f) => (flipped = f)));
      flipped = await b.miniFlipped();
    });

    // Elapsed time counts only while transcribing.
    const tick = setInterval(() => {
      if (status === "transcribing") seconds += 1;
    }, 1000);

    // The window is sized to its content, so the orb stays put and the window grows around it.
    const ro = new ResizeObserver(() => {
      const r = frame.getBoundingClientRect();
      api?.miniResize(Math.ceil(r.width), Math.ceil(r.height), flipped);
    });
    ro.observe(frame);

    // Clicking a button leaves it focused for good, and that used to pin the buttons open. Losing
    // the window's focus ends the visit, the same as the pointer leaving.
    const away = () => {
      hovering = false;
      if (!dragging) (document.activeElement as HTMLElement | null)?.blur();
      refresh();
    };
    window.addEventListener("blur", away);

    return () => {
      window.removeEventListener("blur", away);
      offs.forEach((f) => f());
      clearInterval(tick);
      ro.disconnect();
    };
  });

  // Tell the engine which caption sources are wanted, so a source nobody reads costs nothing.
  $effect(() => {
    api?.setCaptions(...captionFlags()).catch(() => {});
  });

  // Hover or keyboard focus reveals the buttons; they hide again shortly after both are gone.
  // Mouse clicks do not count as focus (`:focus-visible` is keyboard only), so a click never pins them.
  const HIDE_DELAY = 900;
  let dragging = false;
  function refresh() {
    clearTimeout(collapseTimer);
    if (hovering || dragging || frame?.querySelector(":focus-visible")) expanded = true;
    // Linger briefly so a stray mouse move never makes the buttons jump away.
    else collapseTimer = setTimeout(() => (expanded = false), HIDE_DELAY);
  }
  const hover = (on: boolean) => ((hovering = on), refresh());

  const stepText = (d: number) =>
    setPref("captionText", Math.min(CAPTION_TEXT.length - 1, Math.max(0, textStep + d)));

  // Resize handle. It sits at the drawer's outer corner away from the pill: bottom right normally,
  // top right when the drawer is flipped above the pill. Screen coordinates keep the drag steady
  // even though the window moves and resizes under the pointer.
  let start = { x: 0, y: 0, w: 0, h: 0 };
  const fit = (w: number, h: number) => ({
    w: Math.min(CAPTION_LIMITS.maxW, Math.max(CAPTION_LIMITS.minW, Math.round(w))),
    h: Math.min(CAPTION_LIMITS.maxH, Math.max(CAPTION_LIMITS.minH, Math.round(h))),
  });
  function dragStart(e: PointerEvent) {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true;
    start = { x: e.screenX, y: e.screenY, w: drawerW, h: drawerH };
    refresh();
  }
  function dragMove(e: PointerEvent) {
    if (!dragging) return;
    const dy = e.screenY - start.y;
    drag = fit(start.w + (e.screenX - start.x), start.h + (flipped ? -dy : dy));
  }
  function dragEnd() {
    if (!dragging) return;
    dragging = false;
    if (drag) {
      setPref("captionW", drag.w);
      setPref("captionH", drag.h);
    }
    drag = null;
    refresh();
  }
  function handleKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 5 : 20;
    const d = { ArrowRight: [step, 0], ArrowLeft: [-step, 0], ArrowDown: [0, step], ArrowUp: [0, -step] }[e.key];
    if (!d) return;
    e.preventDefault();
    const next = fit(drawerW + d[0], drawerH + d[1]);
    setPref("captionW", next.w);
    setPref("captionH", next.h);
  }
</script>

<main>
  <h1 class="sr-only">SprkNotes overlay</h1>
  <!-- Hover and focus only reveal the buttons; every action is a real button inside. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="frame"
    class:flipped
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
      <div class="drawer" class:flipped style:width="{drawerW}px" style:height="{drawerH}px" style:--cap-size="{CAPTION_TEXT[textStep]}px">
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
      {#if expanded}
        <!-- Hover-only tools. The text-size buttons sit on the edge nearest the pill, the handle on the far one. -->
        <div class="zoom">
          <IconButton label="Smaller text" disabled={textStep === 0} onclick={() => stepText(-1)}>
            <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5" /><path d="M12.3 12.3l4.2 4.2M6.2 8.5h4.6" /></svg>
          </IconButton>
          <IconButton label="Larger text" disabled={textStep === CAPTION_TEXT.length - 1} onclick={() => stepText(1)}>
            <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="8.5" cy="8.5" r="5" /><path d="M12.3 12.3l4.2 4.2M6.2 8.5h4.6M8.5 6.2v4.6" /></svg>
          </IconButton>
        </div>
        <div
          class="handle"
          role="slider"
          tabindex="0"
          aria-valuemin={CAPTION_LIMITS.minH}
          aria-valuemax={CAPTION_LIMITS.maxH}
          aria-valuenow={drawerH}
          aria-label="Resize captions"
          title="Resize"
          onpointerdown={dragStart}
          onpointermove={dragMove}
          onpointerup={dragEnd}
          onpointercancel={dragEnd}
          onkeydown={handleKey}
        >
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M11 5l-6 6M11 8.5l-2.5 2.5" /></svg>
        </div>
      {/if}
      </div>
    {/if}
  </div>
</main>

<style>
  .frame { display: inline-flex; flex-direction: column; align-items: flex-start; gap: 8px; padding: 6px; user-select: none; }
  /* Drawer above the pill: the pill keeps its place and the window grows upward (see resize_mini). */
  .frame.flipped { flex-direction: column-reverse; }
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
  .drawer {
    position: relative;
    box-sizing: border-box;
    border: 1px solid var(--border);
    border-radius: 14px;
    /* Slightly see-through, but dense enough to read over any window. */
    background: color-mix(in srgb, var(--surface) 86%, transparent);
    box-shadow: var(--shadow);
    font-size: var(--cap-size, 17px);
    animation: drawer-in 180ms ease-out;
  }
  .drawer.flipped { animation-name: drawer-in-up; }
  @keyframes drawer-in { from { opacity: 0; transform: translateY(-6px); } }
  @keyframes drawer-in-up { from { opacity: 0; transform: translateY(6px); } }
  .captions {
    box-sizing: border-box;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 0.15em;
    overflow: hidden;
    padding: 0.75em 1em;
    border-radius: inherit;
    /* Older lines fade out as they scroll off the top. */
    mask-image: linear-gradient(to bottom, transparent 0, #000 1.6em);
  }
  .zoom { position: absolute; right: 6px; display: flex; gap: 2px; padding: 2px; border: 1px solid var(--border); border-radius: 999px; background: var(--surface); font-size: 1rem; }
  .drawer:not(.flipped) .zoom { top: 6px; }
  .drawer.flipped .zoom { bottom: 6px; }
  .handle { position: absolute; right: 2px; width: 22px; height: 22px; display: grid; place-items: center; color: var(--muted); touch-action: none; border-radius: 6px; font-size: 1rem; }
  .handle:hover, .handle:focus-visible { color: var(--text); background: var(--surface-2); }
  .handle svg { width: 12px; height: 12px; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; pointer-events: none; }
  .drawer:not(.flipped) .handle { bottom: 2px; cursor: nwse-resize; }
  .drawer.flipped .handle { top: 2px; cursor: nesw-resize; }
  .drawer.flipped .handle svg { transform: scaleY(-1); }
  .cl { margin: 0; line-height: 1.4; }
  .cl.old { color: var(--muted); }
  .cl.live { color: var(--text); }
  .w { animation: wordin 160ms ease-out both; animation-delay: var(--d, 0ms); }
  .w.tent { opacity: 0.72; }
  @keyframes wordin { from { opacity: 0; transform: translateY(2px); } }
  @media (prefers-reduced-motion: reduce) { .w { animation: none; } }
  .cl.hint { color: var(--muted); font-size: 0.9em; }
  .chip { display: inline-block; margin-right: 0.5em; padding: 0 0.5em; border-radius: 999px; font-size: 0.7em; font-weight: 700; background: var(--others-soft); color: var(--others); }
  .chip.me { background: var(--me-soft); color: var(--me); }
  .caret { display: inline-block; width: 2px; height: 1em; margin-left: 3px; vertical-align: -2px; background: var(--accent); animation: blink 1s steps(2) infinite; }
  @keyframes blink { 50% { opacity: 0; } }
  @media (forced-colors: active) { .orb, .drawer, .zoom { border: 1px solid CanvasText; } }
</style>

<script lang="ts">
  // One sound source as a small icon button (Me: microphone, Others: computer sound). Clicking it
  // opens a short menu: which device to transcribe, or turn this source off. The device choice is
  // the same one as in Settings (shared preference), so the two never disagree.
  import { backend } from "./api";
  import { prefs, setPref } from "./prefs.svelte";
  import { menuOptions, deviceLabel } from "./sources";
  import type { Source } from "./types";

  let {
    source,
    on = $bindable(),
    level = null,
    onsource,
  }: {
    source: Source;
    on: boolean;
    /** Input level, 0 to 1, while transcribing; null hides the meter. */
    level?: number | null;
    onsource: (s: Source, on: boolean) => void;
  } = $props();

  // A small meter beside the name: it moves with the sound this source picks up, so a muted
  // or wrong device shows as flat while transcribing. Hidden when off or not transcribing.
  const meter = $derived(on && level !== null ? Math.max(0, Math.min(1, level)) : null);

  let open = $state(false);
  let found = $state<string[]>([]);
  let root: HTMLDivElement;
  let menu = $state<HTMLDivElement>();

  const who = $derived(source === "mic" ? "Me" : "Others");
  const what = $derived(source === "mic" ? "microphone" : "computer sound");
  const key = $derived(source === "mic" ? "micDevice" : "systemDevice");
  const picked = $derived(prefs[key]);
  const options = $derived(menuOptions(found, picked));
  const tip = $derived(`${who}: ${on ? deviceLabel(picked) : "off"}`);

  async function toggleMenu() {
    open = !open;
    if (!open) return;
    // Read the list each time: a headset may have been plugged in since.
    try {
      found = await (await backend).listDevices(source);
    } catch {
      found = [];
    }
    queueMicrotask(() => menu?.querySelector<HTMLElement>("[aria-checked=true], [role^=menuitem]")?.focus());
  }

  function close(refocus = true) {
    open = false;
    if (refocus) root?.querySelector<HTMLElement>("button.src")?.focus();
  }

  async function pick(name: string | null) {
    setPref(key, name);
    try {
      await (await backend).setDevice(source, name);
    } catch {
      /* the next session start applies the saved pick */
    }
    // Picking a device means "transcribe this": a source that was off comes back on.
    if (!on) setOn(true);
    close();
  }

  function setOn(v: boolean) {
    on = v;
    onsource(source, v);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = [...(menu?.querySelectorAll<HTMLElement>("[role^=menuitem]") ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    const next = items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length];
    next?.focus();
  }

  function outside(e: PointerEvent) {
    if (open && !root.contains(e.target as Node)) close(false);
  }
</script>

<svelte:window onpointerdown={outside} />

<div class="wrap" bind:this={root}>
  <button
    class="src {source}"
    class:on
    title={tip}
    aria-label={`${who} (${what}): ${on ? deviceLabel(picked) : "off"}. Choose device`}
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={toggleMenu}
  >
    {#if source === "mic"}
      <svg viewBox="0 0 20 20" aria-hidden="true"><rect x="7" y="2.5" width="6" height="10" rx="3" /><path d="M4.5 9.5a5.5 5.5 0 0011 0M10 15v2.5" /></svg>
    {:else}
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M3.5 7.5h3l4-3.5v12l-4-3.5h-3z" /><path d="M13.5 7a4 4 0 010 6M15.5 5a7 7 0 010 10" /></svg>
    {/if}
    {#if !on}<span class="slash" aria-hidden="true"></span>{/if}
    <span class="who">{who}</span>
    {#if meter !== null}<span class="meter" aria-hidden="true"><span style:transform="scaleY({meter})"></span></span>{/if}
    <svg class="caret" viewBox="0 0 10 10" aria-hidden="true"><path d="M2.5 4l2.5 2.5L7.5 4" /></svg>
  </button>

  {#if open}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="menu" role="menu" tabindex="-1" aria-label={`${who} device`} bind:this={menu} onkeydown={onKey}>
      <p class="head">{who} <span>{what}</span></p>
      {#each options as o (o.value ?? "")}
        <button role="menuitemradio" aria-checked={o.value === picked} class:missing={o.missing} onclick={() => pick(o.value)}>
          <span class="tick" aria-hidden="true">{o.value === picked ? "✓" : ""}</span>
          <span class="name">{o.label}</span>
        </button>
      {/each}
      <hr />
      <button role="menuitem" class="toggle" onclick={() => (setOn(!on), close())}>
        <span class="tick" aria-hidden="true"></span>
        <span class="name">{on ? `Turn off ${who}` : `Turn on ${who}`}</span>
      </button>
    </div>
  {/if}
</div>

<style>
  .wrap { position: relative; display: inline-flex; }
  .src {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    height: 2rem;
    padding: 0 0.45rem 0 0.55rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font: inherit;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 120ms, border-color 120ms;
  }
  .src svg { width: 1rem; height: 1rem; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .src .caret { width: 0.6rem; height: 0.6rem; opacity: 0.7; }
  .src.on.mic { background: var(--me-soft); color: var(--me); border-color: transparent; }
  .src.on.system { background: var(--others-soft); color: var(--others); border-color: transparent; }
  .src:not(.on) .who { text-decoration: line-through; text-decoration-thickness: 1.5px; }
  .meter { position: relative; width: 4px; height: 0.95rem; border-radius: 2px; background: color-mix(in srgb, currentColor 20%, transparent); overflow: hidden; }
  .meter span { position: absolute; inset: 0; background: currentColor; border-radius: 2px; transform-origin: bottom; transition: transform 90ms linear; }
  @media (prefers-reduced-motion: reduce) { .meter span { transition: none; } }
  @media (forced-colors: active) { .meter { background: Canvas; outline: 1px solid CanvasText; } .meter span { background: CanvasText; } }
  .slash { position: absolute; left: 0.5rem; top: 50%; width: 1.15rem; height: 1.5px; background: currentColor; transform: rotate(-45deg); }
  .src:hover { border-color: var(--accent); }
  .src:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 20;
    min-width: 15rem;
    max-width: 22rem;
    padding: 0.35rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow), 0 8px 24px rgb(0 0 0 / 0.12);
    display: flex;
    flex-direction: column;
    animation: pop 120ms ease-out;
  }
  /* Narrow windows stack the sources under the name, at the left edge: open the menu rightward. */
  @media (max-width: 60rem) { .menu { right: auto; left: 0; } }
  @keyframes pop { from { opacity: 0; transform: translateY(-4px); } }
  @media (prefers-reduced-motion: reduce) { .menu { animation: none; } }
  .head { margin: 0.2rem 0.5rem 0.35rem; font-size: 0.78rem; font-weight: 700; }
  .head span { font-weight: 500; color: var(--muted); }
  .menu button {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.42rem 0.5rem;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 0.9rem;
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover, .menu button:focus-visible { background: var(--surface-2); outline: none; }
  .tick { width: 1rem; flex: none; color: var(--accent); font-weight: 700; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .missing .name { color: var(--muted); }
  hr { width: 100%; margin: 0.3rem 0; border: none; border-top: 1px solid var(--border); }
  @media (forced-colors: active) {
    .src.on { border: 2px solid Highlight; }
    .menu { border: 1px solid CanvasText; }
  }
</style>

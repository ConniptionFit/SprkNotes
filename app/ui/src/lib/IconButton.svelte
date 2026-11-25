<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    label,
    onclick,
    pressed,
    primary = false,
    disabled = false,
    children,
  }: {
    /** Short, plain tooltip and screen-reader name, e.g. "Pause". */
    label: string;
    onclick: () => void;
    pressed?: boolean;
    primary?: boolean;
    disabled?: boolean;
    children: Snippet;
  } = $props();
</script>

<button class="icon" class:primary class:on={pressed} title={label} aria-label={label} aria-pressed={pressed} {disabled} {onclick}>
  {@render children()}
</button>

<style>
  .icon {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    transition: background 120ms, color 120ms;
  }
  .icon:hover { background: var(--surface-2); color: var(--text); }
  /* Still hoverable so the tooltip can say why it is off. */
  .icon:disabled { opacity: 0.35; cursor: default; }
  .icon:disabled:hover { background: transparent; color: var(--muted); }
  .icon.on { background: var(--accent-soft); color: var(--accent); }
  .icon.primary { background: var(--accent); color: var(--on-accent); }
  .icon.primary:hover { background: var(--accent-hover); color: var(--on-accent); }
  .icon :global(svg) { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .icon :global(svg .solid) { fill: currentColor; stroke: none; }
</style>

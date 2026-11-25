<script lang="ts">
  import type { Snippet } from "svelte";
  import { MANAGED_TEXT } from "./managed";

  // Wraps a control an administrator may lock. A disabled input shows no tooltip, so the hover
  // text sits on this wrapper, with a small lock that says the same to touch and screen readers.
  let { locked, children }: { locked: boolean; children: Snippet } = $props();
</script>

{#if locked}
  <span class="managed" title={MANAGED_TEXT}>
    <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="3.5" y="7" width="9" height="6.5" rx="1.5" /><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" /></svg>
    <span class="sr-only">{MANAGED_TEXT}</span>
    {@render children()}
  </span>
{:else}
  {@render children()}
{/if}

<style>
  .managed { display: inline-flex; align-items: center; gap: 0.5rem; cursor: not-allowed; }
  svg { width: 0.95rem; height: 0.95rem; fill: none; stroke: var(--muted); stroke-width: 1.4; stroke-linecap: round; flex: none; }
  .managed :global(input), .managed :global(button), .managed :global(label) { cursor: not-allowed; }
  /* A disabled control swallows the hover in some engines; let it fall through to the wrapper. */
  .managed :global(input:disabled), .managed :global(button:disabled) { pointer-events: none; }
</style>

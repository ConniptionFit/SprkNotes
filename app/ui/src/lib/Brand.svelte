<script lang="ts">
  import { prefs } from "./prefs.svelte";
  import throwback from "../assets/throwback-mark.png";
</script>

<span class="brand" aria-hidden="true">
  {#if prefs.throwback}
    <img src={throwback} alt="" width="22" height="22" />
  {:else}
    <svg viewBox="0 0 100 100" width="22" height="22" fill="none" stroke-linecap="round" stroke-linejoin="round">
      <path d="M22 14H78A12 12 0 0 1 90 26V56A12 12 0 0 1 78 68H44L24 88V68H22A12 12 0 0 1 10 56V26A12 12 0 0 1 22 14Z" stroke="var(--accent)" stroke-width="7" />
      <path d="M26 35v12M38 30v22M62 30v22M74 35v12" stroke="var(--accent)" stroke-width="7" />
      <path d="M50 24v34" stroke="var(--live)" stroke-width="7" />
    </svg>
  {/if}
  <span class="word">SprkNotes</span>
</span>

<style>
  .brand { display: inline-flex; align-items: center; gap: 0.45rem; color: var(--muted); user-select: none; }
  img { border-radius: 5px; }
  .word { font-weight: 600; letter-spacing: -0.01em; }
</style>

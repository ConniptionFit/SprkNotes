<script lang="ts">
  import IconButton from "./IconButton.svelte";
  let {
    onlibrary,
    onsettings,
    onsummarize,
    libraryOpen = false,
    settingsOpen = false,
    canSummarize = false,
    summarizeHint = "",
  }: {
    onlibrary: () => void;
    onsettings: () => void;
    onsummarize: () => void;
    libraryOpen?: boolean;
    settingsOpen?: boolean;
    canSummarize?: boolean;
    /** Shown as the tooltip, mostly to say why Summarize is off. */
    summarizeHint?: string;
  } = $props();
</script>

<nav aria-label="More">
  <IconButton label="Library" onclick={onlibrary} pressed={libraryOpen}>
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 4.5h12M4 10h12M4 15.5h8" /></svg>
  </IconButton>
  <IconButton label="Settings" onclick={onsettings} pressed={settingsOpen}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1.08-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1.08 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg>
  </IconButton>
  <IconButton label={canSummarize ? "Summarize" : summarizeHint || "Summarize"} onclick={onsummarize} disabled={!canSummarize}>
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M10 3l1.6 4.4L16 9l-4.4 1.6L10 15l-1.6-4.4L4 9l4.4-1.6z" /></svg>
  </IconButton>
</nav>

<style>
  nav { display: flex; align-items: center; gap: 0.25rem; }
</style>

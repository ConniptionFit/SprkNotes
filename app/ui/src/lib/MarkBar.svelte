<script lang="ts">
  import { NOTE_MAX_CHARS, isMarkKey } from "./marks";

  // Marks and notes during a session (SPEC 5.1). Both land in the transcript as timestamped lines,
  // so the live pane shows them as soon as the backend writes them.
  let {
    onmark,
    onnote,
    disabled = false,
  }: {
    onmark: () => Promise<void> | void;
    onnote: (text: string) => Promise<void> | void;
    disabled?: boolean;
  } = $props();

  let text = $state("");
  let said = $state("");
  let failed = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  function tell(msg: string, bad = false) {
    said = msg;
    failed = bad;
    clearTimeout(timer);
    timer = setTimeout(() => (said = ""), 2500);
  }

  async function mark() {
    if (disabled) return;
    try {
      await onmark();
      tell("Mark added");
    } catch {
      tell("The mark could not be added.", true);
    }
  }

  async function save() {
    const t = text.trim();
    if (disabled || !t) return;
    try {
      await onnote(t);
      text = "";
      tell("Note saved");
    } catch {
      tell("The note could not be saved. It is still in the box.", true);
    }
  }

  function windowKey(e: KeyboardEvent) {
    if (!isMarkKey(e)) return;
    e.preventDefault();
    mark();
  }
</script>

<svelte:window onkeydown={windowKey} />

<form class="marks" onsubmit={(e) => (e.preventDefault(), save())}>
  <button type="button" class="mark" onclick={mark} {disabled} title="Mark this moment (Ctrl+M)" aria-keyshortcuts="Control+M">
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 3.5h9v13l-4.5-3.2-4.5 3.2z" /></svg>
    Mark
  </button>
  <input
    type="text"
    bind:value={text}
    maxlength={NOTE_MAX_CHARS}
    placeholder="Type a note and press Enter"
    aria-label="Note"
    {disabled}
  />
  <button type="submit" class="save" disabled={disabled || !text.trim()}>Save note</button>
  <span class="said" class:failed role="status">{said}</span>
</form>

<style>
  .marks { display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }
  input {
    flex: 1;
    min-width: 12rem;
    padding: 0.5rem 0.85rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }
  input:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  button {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    white-space: nowrap;
    padding: 0.5rem 1rem;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    font-weight: 600;
  }
  button:hover:not(:disabled) { background: var(--accent-soft); }
  button:disabled { opacity: 0.5; }
  .mark { color: var(--accent); }
  svg { width: 1rem; height: 1rem; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linejoin: round; }
  .said { min-width: 6rem; color: var(--muted); font-size: 0.9rem; }
  .said.failed { color: var(--error-text); }
</style>

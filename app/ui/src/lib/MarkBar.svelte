<script lang="ts">
  import { NOTE_MAX_CHARS, isMarkKey } from "./marks";

  // Marks and notes during a session (SPEC 5.1), as one slim bar at the bottom of the transcript.
  // Both land in the transcript as timestamped lines, so the pane shows them as soon as the
  // backend writes them.
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
  let input = $state<HTMLInputElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;

  /** A short word beside the note box ("Mark added", "Copied"). */
  export function tell(msg: string, bad = false) {
    said = msg;
    failed = bad;
    clearTimeout(timer);
    timer = setTimeout(() => (said = ""), 2500);
  }

  /** Start a note with some text already in the box (a line's time), ready to type. */
  export function prefill(start: string) {
    if (disabled) return;
    if (!text.trim()) text = start;
    input?.focus();
    queueMicrotask(() => input?.setSelectionRange(text.length, text.length));
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

<form class="bar" onsubmit={(e) => (e.preventDefault(), save())}>
  <button type="button" class="mark" onclick={mark} {disabled} title="Mark this moment (Ctrl+M)" aria-keyshortcuts="Control+M">
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M5.5 3.5h9v13l-4.5-3.2-4.5 3.2z" /></svg>
    Mark
    <kbd>Ctrl+M</kbd>
  </button>
  <input
    type="text"
    bind:this={input}
    bind:value={text}
    maxlength={NOTE_MAX_CHARS}
    placeholder="Type a note, Enter to save"
    aria-label="Note"
    {disabled}
  />
  {#if text.trim()}
    <button type="submit" class="save" {disabled}>Save</button>
  {/if}
  <span class="said" class:failed role="status">{said}</span>
</form>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.45rem 0.6rem;
    border-top: 1px solid var(--border);
    background: var(--surface);
  }
  input {
    flex: 1;
    min-width: 6rem;
    padding: 0.4rem 0.8rem;
    border: 1px solid transparent;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text);
    font: inherit;
    font-size: 0.92rem;
  }
  input:hover { border-color: var(--border); }
  input:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; background: var(--surface); }
  button {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    white-space: nowrap;
    padding: 0.38rem 0.8rem;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
  }
  button:hover:not(:disabled) { background: var(--accent-soft); }
  button:disabled { opacity: 0.5; cursor: default; }
  .mark { color: var(--accent); }
  kbd { font: inherit; font-size: 0.75rem; font-weight: 500; color: var(--muted); }
  @media (max-width: 40rem) { kbd { display: none; } }
  .save { background: var(--accent); border-color: transparent; color: var(--on-accent); }
  .save:hover:not(:disabled) { background: var(--accent); filter: brightness(1.08); }
  svg { width: 1rem; height: 1rem; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linejoin: round; }
  .said { flex: none; min-width: 0; color: var(--muted); font-size: 0.85rem; white-space: nowrap; }
  .said:empty { display: none; }
  .said.failed { color: var(--error-text); white-space: normal; }
</style>

<script lang="ts">
  // Settings: custom words, names and terms to spell right (SPEC 4.3a). The person's own list is
  // edited here; an administrator's list adds to it and is shown read-only. Used from the next
  // session start. The words stay on this PC and are never logged.
  import { backend } from "./api";
  import type { CustomWords as Words } from "./types";
  import { changed, lastNote, lines } from "./customWords";

  let { sessionActive = false }: { sessionActive?: boolean } = $props();

  let words = $state<Words | null>(null);
  let draft = $state("");
  let message = $state("");
  let error = $state("");
  let busy = $state(false);

  async function load() {
    try {
      words = await (await backend).customWords();
      draft = words.user.join("\n");
    } catch (e) {
      error = String(e);
    }
  }
  load();

  const dirty = $derived(!!words && changed(draft, words.user));
  const note = $derived(lastNote(words));

  async function save() {
    busy = true;
    error = "";
    message = "";
    try {
      words = await (await backend).setCustomWords(lines(draft));
      draft = words.user.join("\n");
      message = sessionActive ? "Saved. The session running now keeps its list; the next one uses this." : "Saved. The next session uses this list.";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="panel">
  <div class="stack">
    <div class="what">
      <h3>Custom words</h3>
      <p class="help">
        Names, medicines and product names you want spelled right. One word or phrase per line. Near misses are
        changed to your spelling; short and everyday words are left alone.
      </p>
    </div>
    {#if words}
      <form
        class="stack"
        onsubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <label class="sr-only" for="custom-words">Custom words, one per line</label>
        <textarea
          id="custom-words"
          rows="4"
          bind:value={draft}
          disabled={busy}
          spellcheck="false"
          autocomplete="off"
          placeholder={"metoprolol\nLisinopril"}
        ></textarea>
        <div class="line">
          <button type="submit" class="primary" disabled={!dirty || busy}>Save</button>
        </div>
      </form>
      {#if words.admin.length}
        <p class="help">Your organization added these too: {words.admin.join(", ")}</p>
      {/if}
      {#if note}<p class="help" role="status">{note}</p>{/if}
    {/if}
    {#if message}<p class="ok" role="status">{message}</p>{/if}
    {#if error}<p class="problem" role="alert">{error}</p>{/if}
  </div>
</div>

<style>
  .panel { border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow); padding: 1.1rem 1.25rem; }
  .stack { display: grid; gap: 0.8rem; }
  .what { display: flex; flex-direction: column; gap: 0.25rem; }
  h3 { margin: 0; font-size: 1.05rem; }
  .help { margin: 0; color: var(--muted); font-size: 0.9rem; max-width: 34rem; overflow-wrap: anywhere; }
  .line { display: flex; gap: 0.5rem; flex-wrap: wrap; align-items: center; }
  textarea { width: 100%; box-sizing: border-box; min-height: 5.5rem; resize: vertical; padding: 0.45rem 0.7rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-2); color: var(--text); font: inherit; }
  textarea:disabled { color: var(--muted); }
  button { padding: 0.45rem 0.9rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); font: inherit; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--surface-2); }
  button:disabled { opacity: 0.5; cursor: default; }
  .primary { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .primary:hover:not(:disabled) { background: var(--accent-hover); }
  .ok { margin: 0; color: var(--ok-text); font-size: 0.9rem; }
  .problem { margin: 0; color: var(--warn); font-size: 0.9rem; }
</style>

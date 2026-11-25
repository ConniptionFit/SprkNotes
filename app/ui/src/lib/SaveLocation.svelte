<script lang="ts">
  // Settings: where transcripts are saved. An administrator can set it (locked here) or give a
  // starting folder; otherwise it is Documents\SprkNotes\Transcripts until the person picks another.
  import { backend } from "./api";
  import type { SaveLocation } from "./types";
  import Managed from "./Managed.svelte";
  import { MANAGED_TEXT } from "./managed";

  let { sessionActive = false }: { sessionActive?: boolean } = $props();

  let loc = $state<SaveLocation | null>(null);
  let draft = $state("");
  let message = $state("");
  let error = $state("");
  let busy = $state(false);

  async function load() {
    try {
      loc = await (await backend).saveLocation();
      draft = loc.chosen ?? loc.path;
    } catch (e) {
      error = String(e);
    }
  }
  load();

  const changed = $derived(!!loc && draft.trim() !== (loc.chosen ?? loc.path));

  async function save(path: string | null) {
    busy = true;
    error = "";
    message = "";
    try {
      loc = await (await backend).setSaveLocation(path);
      draft = loc.chosen ?? loc.path;
      message = sessionActive ? "Saved. The session running now keeps its folder; the next one uses this." : "Saved.";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function open() {
    error = "";
    try {
      await (await backend).libraryOpenFolder("");
    } catch (e) {
      error = String(e);
    }
  }

  const sourceText = $derived(
    !loc
      ? ""
      : loc.source === "policy"
        ? MANAGED_TEXT
        : loc.source === "install"
          ? "Set when SprkNotes was installed. You can change it."
          : loc.source === "default"
            ? "The default folder in Documents."
            : "",
  );
</script>

<div class="panel">
  <div class="stack">
    <div class="what">
      <h3>Save transcripts to</h3>
      <p class="help">
        New transcripts and the Library use this folder. Names like %OneDrive% or %USERPROFILE% work too.
        {sourceText}
      </p>
    </div>
    {#if loc}
      <form
        class="line"
        onsubmit={(e) => {
          e.preventDefault();
          save(draft);
        }}
      >
        <label class="sr-only" for="save-root">Transcripts folder</label>
        <Managed locked={loc.locked}>
          <input id="save-root" bind:value={draft} disabled={loc.locked || busy} spellcheck="false" autocomplete="off" />
        </Managed>
        {#if !loc.locked}
          <button type="submit" class="primary" disabled={!changed || busy || !draft.trim()}>Save</button>
          {#if loc.chosen}<button type="button" disabled={busy} onclick={() => save(null)}>Use default</button>{/if}
        {/if}
        <button type="button" onclick={open}>Open</button>
      </form>
      {#if loc.chosen && loc.chosen !== loc.path}<p class="help">Right now that is {loc.path}</p>{/if}
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
  input { flex: 1 1 16rem; min-width: 0; padding: 0.45rem 0.7rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-2); color: var(--text); font: inherit; }
  input:disabled { color: var(--muted); }
  button { padding: 0.45rem 0.9rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); font: inherit; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--surface-2); }
  button:disabled { opacity: 0.5; cursor: default; }
  .primary { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .primary:hover:not(:disabled) { background: var(--accent-hover); }
  .ok { margin: 0; color: var(--ok-text); font-size: 0.9rem; }
  .problem { margin: 0; color: var(--warn); font-size: 0.9rem; }
</style>

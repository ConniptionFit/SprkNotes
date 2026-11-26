<script lang="ts">
  // Settings: how long transcripts are kept (SPEC 6.6). Older ones go to the Recycle Bin when
  // SprkNotes starts and once a day. An administrator can set it (locked here) or give a starting
  // value; otherwise transcripts are kept forever until the person picks another choice.
  import { backend } from "./api";
  import type { Retention } from "./types";
  import Managed from "./Managed.svelte";
  import Segmented from "./Segmented.svelte";
  import { retentionText, RETENTION_CHOICES } from "./retention";

  let r = $state<Retention | null>(null);
  let message = $state("");
  let error = $state("");
  let busy = $state(false);

  async function load() {
    try {
      r = await (await backend).retention();
    } catch (e) {
      error = String(e);
    }
  }
  load();

  // An administrator's or installer's number that Settings does not offer still shows as a choice.
  const options = $derived(
    [...RETENTION_CHOICES, ...(r && !RETENTION_CHOICES.includes(r.days) ? [r.days] : [])].map((d) => ({
      value: String(d),
      text: retentionText(d),
    })),
  );

  async function pick(v: string) {
    busy = true;
    error = "";
    message = "";
    try {
      r = await (await backend).setRetention(Number(v));
      message =
        r.days === 0
          ? "Saved. Transcripts stay until you delete them."
          : "Saved. Older transcripts go to the Recycle Bin the next time SprkNotes starts, or within a day.";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="panel">
  <div class="stack">
    <div class="row">
      <div class="what">
        <h3>Keep transcripts for</h3>
        <p class="help">
          Older transcripts go to the Recycle Bin when SprkNotes starts and once a day, or are deleted if the Recycle Bin
          can't take them. A transcript that is open or still being written is left alone.
          {#if r?.source === "install"}Set when SprkNotes was installed. You can change it.{/if}
        </p>
      </div>
      {#if r}
        <Managed locked={r.locked}>
          <Segmented
            name="retention"
            label="Keep transcripts for"
            value={String(r.days)}
            {options}
            onchange={pick}
            disabled={r.locked || busy}
          />
        </Managed>
      {/if}
    </div>
    {#if r?.locked}
      <p class="quiet">
        Your organization {r.days === 0 ? "keeps transcripts until they are deleted" : `keeps transcripts for ${retentionText(r.days).toLowerCase()}`}.
      </p>
    {/if}
    {#if message}<p class="ok" role="status">{message}</p>{/if}
    {#if error}<p class="problem" role="alert">{error}</p>{/if}
  </div>
</div>

<style>
  .panel { border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow); padding: 1.1rem 1.25rem; }
  .stack { display: grid; gap: 0.8rem; }
  .row { display: grid; grid-template-columns: 1fr auto; gap: 1.5rem; align-items: center; }
  @media (max-width: 40rem) { .row { grid-template-columns: 1fr; } }
  .what { display: flex; flex-direction: column; gap: 0.25rem; }
  h3 { margin: 0; font-size: 1.05rem; }
  .help { margin: 0; color: var(--muted); font-size: 0.9rem; max-width: 34rem; }
  .quiet { margin: 0; color: var(--muted); font-size: 0.85rem; }
  .ok { margin: 0; color: var(--ok-text); font-size: 0.9rem; }
  .problem { margin: 0; color: var(--warn); font-size: 0.9rem; }
</style>

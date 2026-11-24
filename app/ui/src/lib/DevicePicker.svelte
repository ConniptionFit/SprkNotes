<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./api";
  import { prefs, setPref } from "./prefs.svelte";
  import type { Source } from "./types";

  let { source }: { source: Source } = $props();

  let api = $state.raw<Backend | undefined>();
  let found = $state<string[]>([]);

  const key = $derived(source === "mic" ? "micDevice" : "systemDevice");
  const picked = $derived(prefs[key]);
  const title = $derived(source === "mic" ? "Microphone" : "Speakers and headset (Others)");
  const help = $derived(
    source === "mic"
      ? "Which microphone is transcribed as Me. The system default uses your Windows communications device, so a headset used for calls is picked up. A microphone that is unplugged falls back to the default until it returns."
      : "Which output is transcribed as Others. The system default uses your Windows communications device, where call audio plays. Pick one here if the transcript shows the wrong device.",
  );

  onMount(() => {
    backend.then(async (b) => {
      api = b;
      try {
        found = await b.listDevices(source);
      } catch {
        found = [];
      }
    });
  });

  // A pick that is not plugged in now stays in the list so the choice is not lost silently.
  const options = $derived(picked && !found.includes(picked) ? [...found, picked] : found);

  async function pick(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    const name = v === "" ? null : v;
    setPref(key, name);
    try {
      await api?.setDevice(source, name);
    } catch {
      /* the next session start applies the saved pick */
    }
  }
</script>

<div class="panel">
  <div class="row">
    <div class="what">
      <h3>{title}</h3>
      <p class="help">{help}</p>
    </div>
    <select aria-label={title} value={picked ?? ""} onchange={pick}>
      <option value="">System default</option>
      {#each options as name (name)}
        <option value={name}>{name}</option>
      {/each}
    </select>
  </div>
</div>

<style>
  .panel { border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow); padding: 1.1rem 1.25rem; }
  .row { display: grid; grid-template-columns: 1fr auto; gap: 1.5rem; align-items: center; }
  @media (max-width: 40rem) { .row { grid-template-columns: 1fr; } }
  .what { display: flex; flex-direction: column; gap: 0.25rem; }
  h3 { margin: 0; font-size: 1.05rem; }
  .help { margin: 0; color: var(--muted); font-size: 0.9rem; max-width: 34rem; }
  select { max-width: 18rem; padding: 0.5rem 0.8rem; border: 1px solid var(--border); border-radius: 0.6rem; background: var(--surface-2); color: inherit; font: inherit; }
</style>

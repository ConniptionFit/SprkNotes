<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./api";
  import { prefs, setPref } from "./prefs.svelte";

  let api = $state.raw<Backend | undefined>();
  let inputs = $state<string[]>([]);

  onMount(() => {
    backend.then(async (b) => {
      api = b;
      try {
        inputs = await b.listInputs();
      } catch {
        inputs = [];
      }
    });
  });

  // A pick that is not plugged in now stays in the list so the choice is not lost silently.
  const options = $derived(
    prefs.micDevice && !inputs.includes(prefs.micDevice) ? [...inputs, prefs.micDevice] : inputs,
  );

  async function pick(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    const name = v === "" ? null : v;
    setPref("micDevice", name);
    try {
      await api?.setInput(name);
    } catch {
      /* the next session start applies the saved pick */
    }
  }
</script>

<div class="panel">
  <div class="row">
    <div class="what">
      <h3>Microphone</h3>
      <p class="help">
        Which microphone is transcribed as Me. The system default follows your headset or laptop.
        A microphone that is unplugged falls back to the default until it returns.
      </p>
    </div>
    <select aria-label="Microphone" value={prefs.micDevice ?? ""} onchange={pick}>
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

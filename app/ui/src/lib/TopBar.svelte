<script lang="ts">
  import type { Source, Status } from "./types";
  import Light from "./Light.svelte";
  import SourceButton from "./SourceButton.svelte";

  let {
    name = $bindable(),
    status,
    mic = $bindable(),
    system = $bindable(),
    onrename,
    onsource,
  }: {
    name: string;
    status: Status;
    mic: boolean;
    system: boolean;
    onrename: (n: string) => void;
    onsource: (s: Source, on: boolean) => void;
  } = $props();

</script>

<header>
  <div class="row">
    <input
      class="name"
      aria-label="Session name"
      placeholder="Name this session"
      bind:value={name}
      onchange={() => onrename(name)}
    />
    {#if status !== "ready"}
      <Light {status} size={14} />
    {/if}
  </div>
  <div class="sources" role="group" aria-label="Sound sources">
    <SourceButton source="mic" bind:on={mic} {onsource} />
    <SourceButton source="system" bind:on={system} {onsource} />
  </div>
</header>

<style>
  header { display: flex; flex-direction: column; gap: 0.75rem; }
  .row { display: flex; align-items: center; gap: 1rem; }
  /* Wide windows: the name and the sound sources share one line instead of stacking. */
  @media (min-width: 60rem) {
    header { flex-direction: row; align-items: center; justify-content: space-between; gap: 1.5rem; }
    .row { flex: 1; min-width: 0; }
  }
  .name {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: 1.6rem;
    font-weight: 650;
    letter-spacing: -0.01em;
    color: var(--text);
    background: transparent;
    border: 1px solid transparent;
    border-radius: 8px;
    padding: 0.2rem 0.5rem;
    margin-left: -0.5rem;
    text-overflow: ellipsis;
  }
  .name::placeholder { color: var(--muted); font-weight: 500; }
  .name:hover { border-color: var(--border); }
  .name:focus { border-color: var(--accent); background: var(--surface); outline: none; }


  .sources { display: flex; gap: 0.4rem; flex: none; }
</style>

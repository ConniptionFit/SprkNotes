<script lang="ts">
  import type { Source, Status } from "./types";
  import Light from "./Light.svelte";
  import SourceButton from "./SourceButton.svelte";

  let {
    name = $bindable(),
    status,
    mic = $bindable(),
    system = $bindable(),
    levels = null,
    elapsed = "",
    onrename,
    onsource,
  }: {
    name: string;
    status: Status;
    mic: boolean;
    system: boolean;
    /** Me and Others input levels, 0 to 1; null when nothing is being transcribed. */
    levels?: [number, number] | null;
    /** Session time so far ("12:41"), shown beside the status while a session runs. */
    elapsed?: string;
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
      <p class="status" class:paused={status === "paused"}>
        <Light {status} size={12} />
        <span class="word" aria-hidden="true">{status === "paused" ? "Paused" : "Listening"}</span>
        <time title="Time transcribed so far">{elapsed}</time>
      </p>
    {/if}
  </div>
  <div class="sources" role="group" aria-label="Sound sources">
    <SourceButton source="mic" bind:on={mic} level={levels?.[0] ?? null} {onsource} />
    <SourceButton source="system" bind:on={system} level={levels?.[1] ?? null} {onsource} />
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


  .sources { display: flex; gap: 0.5rem; flex: none; }
  /* Session status: the light, what is happening, and how long it has run. */
  .status {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    flex: none;
    margin: 0;
    padding: 0.3rem 0.75rem 0.3rem 0.6rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--live) 12%, transparent);
    font-size: 0.88rem;
    font-weight: 650;
  }
  .status.paused { background: var(--warn-soft); }
  .status .word { color: var(--text); }
  .status time { color: var(--muted); font-weight: 600; font-variant-numeric: tabular-nums; }
</style>

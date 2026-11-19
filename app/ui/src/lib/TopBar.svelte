<script lang="ts">
  import type { Source, Status } from "./types";

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

  const label: Record<Status, string> = {
    ready: "Ready",
    transcribing: "Transcribing",
    paused: "Paused",
  };
</script>

<header>
  <input
    class="name"
    aria-label="Session name"
    placeholder="Session name"
    bind:value={name}
    onchange={() => onrename(name)}
  />
  <label class="toggle">
    <input type="checkbox" bind:checked={mic} onchange={() => onsource("mic", mic)} />
    Me (mic)
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={system} onchange={() => onsource("system", system)} />
    Others (system)
  </label>
  <span class="status {status}" role="status">{label[status]}</span>
</header>

<style>
  header { display: flex; gap: 1rem; align-items: center; flex-wrap: wrap; }
  .name { flex: 1; min-width: 12rem; font-size: 1.1rem; padding: 0.4rem 0.6rem; }
  .toggle { display: inline-flex; gap: 0.35rem; align-items: center; }
  .status { padding: 0.2rem 0.8rem; border-radius: 999px; background: var(--line); }
  .status.transcribing { background: var(--accent); color: white; }
  .status.paused { background: var(--warn); color: black; }
</style>

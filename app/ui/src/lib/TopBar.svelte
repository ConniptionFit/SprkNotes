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

  const label: { [K in Status]: string } = {
    ready: "Ready",
    transcribing: "Transcribing",
    paused: "Paused",
  };
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
    <span class="status {status}" role="status"><i></i>{label[status]}</span>
  </div>
  <div class="sources" role="group" aria-label="Sound sources">
    <label class="chip me" class:on={mic}>
      <input type="checkbox" bind:checked={mic} onchange={() => onsource("mic", mic)} />
      <span class="dot"></span>
      <span class="who">Me</span><span class="what">your microphone</span>
    </label>
    <label class="chip others" class:on={system}>
      <input type="checkbox" bind:checked={system} onchange={() => onsource("system", system)} />
      <span class="dot"></span>
      <span class="who">Others</span><span class="what">computer sound</span>
    </label>
  </div>
</header>

<style>
  header { display: flex; flex-direction: column; gap: 0.75rem; }
  .row { display: flex; align-items: center; gap: 1rem; }
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

  .status {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.3rem 0.85rem;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 0.9rem;
    font-weight: 600;
    white-space: nowrap;
  }
  .status i { width: 0.55rem; height: 0.55rem; border-radius: 50%; background: currentColor; }
  .status.transcribing { background: var(--accent-soft); color: var(--accent); }
  .status.transcribing i { background: var(--live); animation: pulse 1.6s ease-in-out infinite; }
  .status.paused { background: var(--warn-soft); color: var(--warn); }
  @keyframes pulse { 50% { opacity: 0.35; transform: scale(0.8); } }

  .sources { display: flex; gap: 0.5rem; flex-wrap: wrap; }
  .chip {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.85rem 0.4rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    cursor: pointer;
    user-select: none;
    transition: background 120ms, border-color 120ms;
  }
  .chip input { position: absolute; opacity: 0; inset: 0; margin: 0; cursor: pointer; }
  .chip:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .dot { width: 0.6rem; height: 0.6rem; border-radius: 50%; border: 2px solid currentColor; }
  .who { font-weight: 600; }
  .what { font-size: 0.85rem; }
  .chip.on.me { background: var(--me-soft); color: var(--me); border-color: transparent; }
  .chip.on.others { background: var(--others-soft); color: var(--others); border-color: transparent; }
  .chip.on .dot { background: currentColor; }
</style>

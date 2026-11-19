<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./lib/api";
  import type { Line, Source, Status } from "./lib/types";
  import TopBar from "./lib/TopBar.svelte";
  import Controls from "./lib/Controls.svelte";
  import LivePane from "./lib/LivePane.svelte";

  let api: Backend | undefined;
  let name = $state("");
  let status = $state<Status>("ready");
  let mic = $state(true);
  let system = $state(true);
  let lines = $state<Line[]>([]);

  onMount(() => {
    const offs: Array<() => void> = [];
    backend.then(async (b) => {
      api = b;
      offs.push(await b.onLine((l) => (lines = [...lines, l])));
      offs.push(await b.onStatus((s) => (status = s)));
    });
    return () => offs.forEach((f) => f());
  });

  async function start() {
    lines = [];
    await api?.start(name);
  }
</script>

<main>
  <TopBar
    bind:name
    {status}
    bind:mic
    bind:system
    onrename={(n) => api?.rename(n)}
    onsource={(s: Source, on: boolean) => api?.setSource(s, on)}
  />
  <Controls
    {status}
    onstart={start}
    onpause={() => api?.pause()}
    onresume={() => api?.resume()}
    onend={() => api?.end()}
  />
  <LivePane {lines} />
  <footer>
    <button disabled title="Arrives with the library">Library</button>
    <button disabled title="Arrives with settings">Settings</button>
    <button disabled title="Available after a session ends">Summarize</button>
  </footer>
</main>

<style>
  main { display: flex; flex-direction: column; gap: 0.75rem; height: 100vh; box-sizing: border-box; padding: 1rem; }
  footer { display: flex; gap: 0.5rem; justify-content: flex-end; }
</style>

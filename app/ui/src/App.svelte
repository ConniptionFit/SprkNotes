<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./lib/api";
  import type { Line, Source, Status } from "./lib/types";
  import TopBar from "./lib/TopBar.svelte";
  import Controls from "./lib/Controls.svelte";
  import LivePane from "./lib/LivePane.svelte";
  import Footer from "./lib/Footer.svelte";

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
  <LivePane {lines} {status} />
  <div class="dock">
    <Footer />
    <Controls
      {status}
      onstart={start}
      onpause={() => api?.pause()}
      onresume={() => api?.resume()}
      onend={() => api?.end()}
    />
  </div>
</main>

<style>
  main {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    height: 100vh;
    padding: 1.25rem 1.5rem 1rem;
    max-width: 62rem;
    margin: 0 auto;
  }
  .dock { display: flex; align-items: center; justify-content: space-between; gap: 1rem; flex-wrap: wrap; }
</style>

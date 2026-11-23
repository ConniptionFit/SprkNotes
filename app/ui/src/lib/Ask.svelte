<script lang="ts">
  import type { Backend } from "./api";
  import type { ActionOut, AssistantFeature, AssistantRequest, AssistantResponse, Line } from "./types";

  let {
    api,
    lines,
    features,
    onclose,
  }: {
    api: Backend | undefined;
    lines: Line[];
    features: AssistantFeature[];
    onclose: () => void;
  } = $props();

  type Entry = { id: number; label: string; res: AssistantResponse | null; error: string };
  // Kept in memory only. Closing the window or ending the app forgets all of it.
  let entries = $state<Entry[]>([]);
  let next = 0;
  let busy = $state(false);
  let question = $state("");
  let agenda = $state("");
  let agendaOpen = $state(false);
  let pinLabel = $state("");
  let lineId = $state<number | null>(null);

  // A few ready-made questions. Each one is sent as an ordinary typed question.
  const PRESETS = ["What was decided?", "What is still open?", "Who owns what?"];

  const has = (f: AssistantFeature) => features.includes(f);
  const statements = $derived(lines.filter((l) => l.kind === "statement").slice(-8).reverse());
  const stamp = (id: number) => lines.find((l) => l.id === id)?.t ?? "";

  const ERRORS: { [code: string]: string } = {
    off: "Ask Sprk is off. Turn it on in Settings.",
    disabled: "That feature is off in Settings.",
    noModel: "No assistant model is installed yet. Add one in Settings.",
    notEnoughMemory: "Not enough free memory right now. Close an app and try again.",
    busy: "Still working on the last question.",
    cancelled: "Stopped.",
    failed: "That did not work. Try again.",
  };

  async function run(label: string, req: AssistantRequest) {
    if (!api || busy) return;
    busy = true;
    const e: Entry = { id: next++, label, res: null, error: "" };
    entries = [e, ...entries];
    try {
      const res = await api.assistantRun(req);
      entries = entries.map((x) => (x.id === e.id ? { ...x, res } : x));
    } catch (err) {
      const code = String(err);
      entries = entries.map((x) => (x.id === e.id ? { ...x, error: ERRORS[code] ?? ERRORS.failed } : x));
    } finally {
      busy = false;
    }
  }

  function ask() {
    const q = question.trim();
    if (!q) return;
    question = "";
    run(q, { op: "ask", question: q });
  }

  function who(a: ActionOut) {
    return a.ownerKind === "me" ? "Me" : a.ownerKind === "named" ? (a.ownerName ?? "Someone") : "Owner not stated";
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      /* nothing to do: the text is still on screen */
    }
  }

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={key} />

{#snippet cites(ids: number[])}
  {#each ids as c (c)}
    {#if stamp(c)}<span class="cite" title="Line from the transcript">{stamp(c)}</span>{/if}
  {/each}
{/snippet}

<section class="ask" aria-label="Ask Sprk">
  <header>
    <h2>Ask Sprk</h2>
    <button class="x" onclick={onclose} aria-label="Close">×</button>
  </header>

  <form class="inline ask-box" onsubmit={(e) => (e.preventDefault(), ask())}>
    <input
      aria-label="Your question"
      placeholder="Type a question about this transcript"
      maxlength="500"
      disabled={!has("ask")}
      bind:value={question}
    />
    <button class="go" disabled={busy || !has("ask") || !question.trim()}>Ask</button>
  </form>
  {#if !has("ask")}<p class="muted note">Turn on "Ask a question" in Settings to type your own.</p>{/if}

  <div class="chips" role="group" aria-label="Quick questions">
    {#if has("ask")}
      {#each PRESETS as q (q)}
        <button disabled={busy} onclick={() => run(q, { op: "ask", question: q })}>{q}</button>
      {/each}
    {/if}
    {#if has("recap")}<button disabled={busy} onclick={() => run("What did I miss?", { op: "recap", minutes: 5 })}>What did I miss?</button>{/if}
    {#if has("actions")}<button disabled={busy} onclick={() => run("Action items", { op: "actions" })}>Action items</button>{/if}
    {#if has("commitments")}<button disabled={busy} onclick={() => run("My commitments", { op: "commitments" })}>My commitments</button>{/if}
    {#if has("parkinglot")}<button disabled={busy} onclick={() => run("Parking lot", { op: "parking" })}>Parking lot</button>{/if}
    {#if has("pins")}<button disabled={busy} onclick={() => run("Pinned moments", { op: "pins" })}>Pinned moments</button>{/if}
    {#if has("agenda")}<button disabled={busy} onclick={() => (agendaOpen = !agendaOpen)}>Agenda check</button>{/if}
    {#if has("asked")}<button disabled={busy} onclick={() => run("Was I asked something?", { op: "asked" })}>Was I asked something?</button>{/if}
    {#if has("summary")}<button disabled={busy} onclick={() => run("Summary so far", { op: "summary" })}>Summary so far</button>{/if}
    {#if has("rolling")}<button disabled={busy} onclick={() => run("Earlier catch-ups", { op: "catchups" })}>Earlier catch-ups</button>{/if}
    {#if has("prep")}<button disabled={busy} onclick={() => run("Next meeting", { op: "prep" })}>Next meeting</button>{/if}
  </div>

  {#if has("pins")}
    <form class="inline" onsubmit={(e) => (e.preventDefault(), run("Pin this moment", { op: "pinNow", label: pinLabel }), (pinLabel = ""))}>
      <input aria-label="Pin label" placeholder="Pin this moment (optional note)" bind:value={pinLabel} />
      <button disabled={busy || lines.length === 0}>Pin</button>
    </form>
  {/if}

  {#if agendaOpen && has("agenda")}
    <form class="inline" onsubmit={(e) => (e.preventDefault(), run("Agenda check", { op: "agenda", items: agenda.split("\n").map((s) => s.trim()).filter(Boolean) }))}>
      <textarea aria-label="Agenda, one item per line" rows="3" placeholder="One agenda item per line" bind:value={agenda}></textarea>
      <button disabled={busy || !agenda.trim()}>Check</button>
    </form>
  {/if}

  {#if (has("explain") || has("shorten")) && statements.length > 0}
    <div class="inline">
      <select aria-label="A line from the transcript" bind:value={lineId}>
        <option value={null}>Pick a recent line…</option>
        {#each statements as l (l.id)}
          <option value={l.id}>{l.t} {l.kind === "statement" ? l.text.slice(0, 60) : ""}</option>
        {/each}
      </select>
      {#if has("explain")}<button disabled={busy || lineId === null} onclick={() => lineId !== null && run("Explain this", { op: "explain", lineId })}>Explain this</button>{/if}
      {#if has("shorten")}<button disabled={busy || lineId === null} onclick={() => lineId !== null && run("Say it again, short", { op: "shorten", fromId: lineId, toId: lineId })}>Say it short</button>{/if}
    </div>
  {/if}

  {#if busy}
    <p class="working" role="status">Working on it… <button class="link" onclick={() => api?.assistantCancel()}>Stop</button></p>
  {/if}

  <div class="results" aria-live="polite">
    {#each entries as e (e.id)}
      <article>
        <h3>{e.label}</h3>
        {#if e.error}
          <p class="err">{e.error}</p>
        {:else if !e.res}
          <p class="muted">…</p>
        {:else if e.res.kind === "recap"}
          {#if e.res.nothingSaid}<p class="muted">No one has spoken in that time.</p>
          {:else if e.res.bullets.length === 0}<p class="muted">Nothing I could confirm from the transcript.</p>{/if}
          <ul>{#each e.res.bullets as b}<li>{b.text} {@render cites(b.cites)}</li>{/each}</ul>
        {:else if e.res.kind === "actions" || e.res.kind === "commitments"}
          {#if e.res.items.length === 0}<p class="muted">No action items found.</p>{/if}
          <ul>{#each e.res.items as a}<li>{a.task} <span class="meta">({who(a)}, {a.due ?? "due date not stated"})</span> {@render cites(a.cites)}</li>{/each}</ul>
        {:else if e.res.kind === "answer"}
          {#if e.res.state === "ok"}<p>{e.res.text} {@render cites(e.res.cites)}</p>
          {:else if e.res.state === "notStated"}<p class="muted">The transcript does not say.</p>
          {:else}<p class="muted">I could not confirm an answer from the transcript.</p>{/if}
        {:else if e.res.kind === "title"}
          <p>{e.res.text ?? "Not enough said yet."}</p>
        {:else if e.res.kind === "agenda"}
          <ul class="plain">{#each e.res.items as a}<li>{a.covered ? "Covered" : "Not yet"}: {a.item} {@render cites(a.cites)}</li>{/each}</ul>
        {:else if e.res.kind === "pins"}
          {#if e.res.pins.length === 0}<p class="muted">Nothing pinned yet.</p>{/if}
          <ul class="plain">
            {#each e.res.pins as p}
              <li>{p.t} {p.label || "Pinned"} <button class="link" onclick={() => run("Unpin", { op: "unpin", lineId: p.lineId })}>Remove</button></li>
            {/each}
          </ul>
        {:else if e.res.kind === "parking"}
          {#if e.res.items.length === 0}<p class="muted">Nothing was set aside.</p>{/if}
          <ul class="plain">{#each e.res.items as p}<li><span class="cite">{p.t}</span> {p.text}</li>{/each}</ul>
        {:else if e.res.kind === "asked"}
          {#if e.res.items.length === 0}<p class="muted">No one has asked you anything since you last spoke.</p>{/if}
          <ul class="plain">{#each e.res.items as p}<li><span class="cite">{p.t}</span> {p.text}</li>{/each}</ul>
        {:else if e.res.kind === "catchups"}
          {#if e.res.items.length === 0}<p class="muted">The first one is written after 10 minutes.</p>{/if}
          {#each e.res.items as c}
            <h3>{c.from} to {c.to}</h3>
            <ul>{#each c.bullets as b}<li>{b.text} {@render cites(b.cites)}</li>{/each}</ul>
          {/each}
        {:else if e.res.kind === "prep"}
          <pre>{e.res.markdown}</pre>
          <button class="link" onclick={() => e.res?.kind === "prep" && copy(e.res.markdown)}>Copy</button>
        {/if}
      </article>
    {/each}
  </div>
  <p class="foot">Runs on this PC. Nothing is saved or sent anywhere.</p>
</section>

<style>
  .ask {
    position: absolute; right: clamp(1.5rem, 4vw, 4rem); bottom: 4.5rem; z-index: 10; width: min(30rem, calc(100vw - 3rem));
    max-height: 70vh; overflow-y: auto; display: grid; gap: 0.6rem; padding: 0.9rem 1rem;
    border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow);
  }
  header { display: flex; justify-content: space-between; align-items: center; }
  h2 { margin: 0; font-size: 1.1rem; }
  h3 { margin: 0 0 0.2rem; font-size: 0.9rem; color: var(--muted); font-weight: 600; }
  .x { border: none; background: transparent; font-size: 1.3rem; color: var(--muted); cursor: pointer; }
  .chips { display: flex; flex-wrap: wrap; gap: 0.4rem; }
  .chips button, .inline button {
    padding: 0.35rem 0.75rem; border: 1px solid var(--border); border-radius: 999px; background: var(--surface); color: var(--text); cursor: pointer;
  }
  .chips button:hover:not(:disabled), .inline button:hover:not(:disabled) { background: var(--accent-soft); }
  button:disabled { opacity: 0.5; cursor: default; }
  .inline { display: flex; gap: 0.4rem; align-items: flex-start; }
  .inline input, .inline textarea, .inline select {
    flex: 1; min-width: 0; font: inherit; padding: 0.4rem 0.6rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text);
  }
  .ask-box input { padding: 0.55rem 0.75rem; }
  .ask-box .go { background: var(--accent); border-color: transparent; color: var(--on-accent); font-weight: 600; }
  .ask-box .go:hover:not(:disabled) { background: var(--accent-hover); }
  .note { margin: 0; font-size: 0.85rem; }
  .results { display: grid; gap: 0.7rem; }
  article { padding-top: 0.5rem; border-top: 1px solid var(--border); }
  article p, article ul { margin: 0; }
  ul { padding-left: 1.1rem; display: grid; gap: 0.25rem; }
  ul.plain { list-style: none; padding-left: 0; }
  pre { margin: 0; white-space: pre-wrap; font: inherit; }
  .cite { display: inline-block; padding: 0 0.4rem; border-radius: 999px; background: var(--accent-soft); font-size: 0.8rem; color: var(--muted); }
  .meta, .muted, .foot { color: var(--muted); }
  .err { color: var(--warn); }
  .working { margin: 0; color: var(--muted); }
  .foot { margin: 0; font-size: 0.8rem; }
  .link { border: none; background: transparent; color: var(--accent); cursor: pointer; padding: 0 0.25rem; text-decoration: underline; }
</style>

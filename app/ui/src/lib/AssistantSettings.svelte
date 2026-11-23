<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./api";
  import type { AssistantFeature, AssistantStatus } from "./types";
  import Segmented from "./Segmented.svelte";
  import { prefs, setPref } from "./prefs.svelte";

  let api = $state.raw<Backend | undefined>();
  let st = $state<AssistantStatus | null>(null);

  const CORE: { key: AssistantFeature; text: string; help: string }[] = [
    { key: "title", text: "Suggest a title", help: "Names the session once enough has been said." },
    { key: "recap", text: "What did I miss?", help: "A short recap of the last few minutes." },
    { key: "actions", text: "Action items", help: "A list of tasks, who has them, and when they are due." },
    { key: "ask", text: "Ask a question", help: "Answers from the transcript, with the lines it used." },
  ];
  const MORE: { key: AssistantFeature; text: string; help: string }[] = [
    { key: "agenda", text: "Agenda check", help: "Type your agenda and see what has been covered." },
    { key: "commitments", text: "My commitments", help: "What you said you would do." },
    { key: "pins", text: "Pin a moment", help: "Mark a spot to come back to. No model needed." },
    { key: "parkinglot", text: "Parking lot", help: "Topics someone set aside. No model needed." },
    { key: "explain", text: "Explain this", help: "Says what a chosen line meant, from the transcript." },
    { key: "shorten", text: "Say it again, short", help: "One short sentence for a chosen line." },
    { key: "prep", text: "Next-meeting prep", help: "Open items, set-aside topics and pins in one list." },
    { key: "asked", text: "Was I asked something?", help: "Shows questions from others since you last spoke. No model needed." },
    { key: "summary", text: "Summary of the meeting so far", help: "A short summary of everything said, written on this PC." },
    { key: "rolling", text: "Catch-ups while you work", help: "Writes a short catch-up every 10 minutes in the background, on this PC." },
  ];

  async function refresh() {
    try {
      st = (await api?.assistantStatus()) ?? null;
    } catch {
      st = null;
    }
  }

  async function push() {
    await api?.assistantSet(prefs.assistantOn, prefs.assistantTier, prefs.assistantFeatures);
    await refresh();
  }

  onMount(() => {
    backend.then(async (b) => {
      api = b;
      await refresh();
    });
  });

  function toggleFeature(key: AssistantFeature, on: boolean) {
    const rest = prefs.assistantFeatures.filter((k) => k !== key);
    setPref("assistantFeatures", on ? [...rest, key] : rest);
    push();
  }

  const allowed = (k: AssistantFeature) => !st || st.allowedFeatures.includes(k);
  const statusText = $derived(
    !st ? "" :
    st.state === "off" ? (st.locked ? "Turned off by your organization." : "Off") :
    st.state === "noModel" ? "No assistant model is installed yet." :
    st.state === "notEnoughMemory" ? "Not enough free memory right now." :
    "Ready",
  );
</script>

<div class="panel">
  <div class="stack">
    <div class="row">
      <div class="what">
        <h3>Ask Sprk</h3>
        <p class="help">
          Ask questions about your transcript while it is still going. It runs on this PC, uses the
          computer lightly, and nothing it says is saved. Off until you turn it on.
        </p>
        {#if statusText}<p class="status" class:missing={st?.state !== "ready" && st?.state !== "off"}>{statusText}</p>{/if}
      </div>
      <label class="switch">
        <input
          type="checkbox"
          role="switch"
          checked={prefs.assistantOn}
          disabled={st?.locked}
          onchange={(e) => (setPref("assistantOn", e.currentTarget.checked), push())}
        />
        <span class="track" aria-hidden="true"></span>
        <span class="sr-only">Turn on Ask Sprk</span>
      </label>
    </div>

    {#if prefs.assistantOn}
      <div class="row">
        <div class="what">
          <p class="plain">Size</p>
          <p class="help">Light is the default. Bigger answers better but is slower and needs more memory.</p>
        </div>
        <Segmented
          name="assistant-tier"
          label="Assistant size"
          value={prefs.assistantTier}
          options={[
            { value: "titles", text: "Titles only" },
            { value: "light", text: "Light" },
            { value: "standard", text: "Standard" },
          ]}
          onchange={(v: "titles" | "light" | "standard") => (setPref("assistantTier", v), push())}
        />
      </div>

      <fieldset>
        <legend>Included</legend>
        {#each CORE as f (f.key)}
          <label class="opt" class:off={!allowed(f.key)}>
            <input type="checkbox" checked={prefs.assistantFeatures.includes(f.key)} disabled={!allowed(f.key)} onchange={(e) => toggleFeature(f.key, e.currentTarget.checked)} />
            <span><b>{f.text}</b> <span class="help">{f.help}</span></span>
          </label>
        {/each}
      </fieldset>
      <fieldset>
        <legend>More, all off until you choose</legend>
        {#each MORE as f (f.key)}
          <label class="opt" class:off={!allowed(f.key)}>
            <input type="checkbox" checked={prefs.assistantFeatures.includes(f.key)} disabled={!allowed(f.key)} onchange={(e) => toggleFeature(f.key, e.currentTarget.checked)} />
            <span><b>{f.text}</b> <span class="help">{f.help}</span></span>
          </label>
        {/each}
      </fieldset>
    {/if}
  </div>
</div>

<style>
  h3 { margin: 0; font-size: 1.05rem; }
  .panel { border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow); padding: 1.1rem 1.25rem; }
  .row { display: grid; grid-template-columns: 1fr auto; gap: 1.5rem; align-items: center; }
  @media (max-width: 40rem) { .row { grid-template-columns: 1fr; } }
  .what { display: flex; flex-direction: column; gap: 0.25rem; }
  .stack { display: grid; gap: 1.1rem; }
  .plain { margin: 0; font-weight: 600; }
  .status { margin: 0; font-weight: 600; color: var(--ok-text); }
  .status.missing { color: var(--warn); }
  .help { margin: 0; color: var(--muted); font-size: 0.9rem; max-width: 34rem; }
  fieldset { border: none; padding: 0; margin: 0; display: grid; gap: 0.5rem; }
  legend { font-weight: 600; padding: 0; margin-bottom: 0.4rem; }
  .opt { display: flex; gap: 0.6rem; align-items: flex-start; cursor: pointer; }
  .opt.off { opacity: 0.5; cursor: default; }
  .switch { position: relative; display: inline-block; width: 46px; height: 26px; }
  .switch input { position: absolute; opacity: 0; inset: 0; width: 100%; height: 100%; margin: 0; cursor: pointer; z-index: 1; }
  .track { position: absolute; inset: 0; border-radius: 13px; background: var(--surface-2); border: 1px solid var(--border); transition: background 120ms; }
  .track::after { content: ""; position: absolute; top: 3px; left: 3px; width: 18px; height: 18px; border-radius: 50%; background: var(--muted); transition: transform 120ms, background 120ms; }
  input:checked + .track { background: var(--accent); border-color: transparent; }
  input:checked + .track::after { transform: translateX(20px); background: var(--on-accent); }
  .switch:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 13px; }
</style>

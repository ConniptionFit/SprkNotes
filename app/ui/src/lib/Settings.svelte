<script lang="ts">
  import type { DownloadProgress, ModelStatus } from "./types";
  import ModelDownload from "./ModelDownload.svelte";
  import Segmented from "./Segmented.svelte";
  import AssistantSettings from "./AssistantSettings.svelte";
  import AppUpdates from "./AppUpdates.svelte";
  import { prefs, setPref, type CaptionWho, type CaptionDelay, type Theme } from "./prefs.svelte";

  let {
    model,
    progress,
    updateAvailable,
    sessionActive,
    checkMessage,
    ondownload,
    oncheck,
    onback,
  }: {
    model: ModelStatus;
    progress: DownloadProgress | null;
    updateAvailable: boolean;
    sessionActive: boolean;
    checkMessage: string;
    ondownload: () => void;
    oncheck: () => void;
    onback: () => void;
  } = $props();

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") onback();
  }
</script>

<svelte:window onkeydown={key} />

<section class="settings" aria-labelledby="settings-title">
  <header>
    <button class="back" onclick={onback} aria-label="Back to transcript">
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M12 4l-6 6 6 6" /></svg>
    </button>
    <h2 id="settings-title">Settings</h2>
  </header>

  <div class="panel">
    <div class="row">
      <div class="what">
        <h3>Transcription model</h3>
        <p class="status" class:missing={!model.ready}>
          {model.ready ? "Installed and ready" : "Not installed yet"}
        </p>
        <p class="help">
          The model turns speech into text on this PC. Audio never leaves your computer.
          {#if model.customSource}Your organization provides the download.{/if}
        </p>
      </div>
      <ModelDownload
        {model}
        {progress}
        {updateAvailable}
        {sessionActive}
        {checkMessage}
        {ondownload}
        {oncheck}
      />
    </div>
  </div>

  <AppUpdates />

  <AssistantSettings />

  <div class="panel">
    <div class="row">
      <div class="what">
        <h3>Appearance</h3>
        <p class="help">The overlay is dark by default. Choose Light to make the overlay light too.</p>
      </div>
      <Segmented
        name="theme"
        label="Appearance"
        value={prefs.theme}
        options={[
          { value: "system", text: "System" },
          { value: "light", text: "Light" },
          { value: "dark", text: "Dark" },
        ]}
        onchange={(v: Theme) => setPref("theme", v)}
      />
    </div>
  </div>

  <div class="panel">
    <div class="stack">
      <div class="row">
        <div class="what">
          <h3>Live captions</h3>
          <p class="help">
            Show words on the overlay while people are still speaking. Captions are never saved and the
            transcript is unchanged.
          </p>
        </div>
        <label class="switch">
          <input
            type="checkbox"
            role="switch"
            checked={prefs.captionsOn}
            onchange={(e) => setPref("captionsOn", e.currentTarget.checked)}
          />
          <span class="track" aria-hidden="true"></span>
          <span class="sr-only">Show live captions on the overlay</span>
        </label>
      </div>
      <div class="row">
        <div class="what">
          <p class="plain">Live line in the transcript window</p>
          <p class="help">The newest words grow at the end of the transcript before they settle into a line.</p>
        </div>
        <label class="switch">
          <input
            type="checkbox"
            role="switch"
            checked={prefs.liveLine}
            onchange={(e) => setPref("liveLine", e.currentTarget.checked)}
          />
          <span class="track" aria-hidden="true"></span>
          <span class="sr-only">Show the live line in the transcript window</span>
        </label>
      </div>
      <div class="row">
        <div class="what">
          <p class="plain">Live text delay</p>
          <p class="help">A short wait lets words settle, so they change less. Steady is under one second.</p>
        </div>
        <Segmented
          name="caption-delay"
          label="How long live text waits before it appears"
          value={prefs.captionDelay}
          options={[
            { value: "fast", text: "Fast" },
            { value: "steady", text: "Steady" },
            { value: "smooth", text: "Smoothest" },
          ]}
          onchange={(v: CaptionDelay) => setPref("captionDelay", v)}
        />
      </div>
      <div class="row">
        <div class="what">
          <p class="plain">Whose words</p>
          <p class="help">Others only is the default, and the quickest.</p>
        </div>
        <Segmented
          name="caption-who"
          label="Whose words appear in captions"
          value={prefs.captionsWho}
          options={[
            { value: "others", text: "Others only" },
            { value: "both", text: "Others and Me" },
          ]}
          onchange={(v: CaptionWho) => setPref("captionsWho", v)}
        />
      </div>
    </div>
  </div>

  <div class="panel">
    <div class="stack">
      <h3>Misc</h3>
    <div class="row">
      <div class="what">
        <p class="plain">Throwback Mode</p>
        <p class="help">Get a blast from the past. Same app, a little vintage.</p>
      </div>
      <label class="switch">
        <input
          type="checkbox"
          role="switch"
          checked={prefs.throwback}
          onchange={(e) => setPref("throwback", e.currentTarget.checked)}
        />
        <span class="track" aria-hidden="true"></span>
        <span class="sr-only">Turn on Throwback Mode</span>
      </label>
    </div>
    </div>
  </div>
</section>

<style>
  .settings { flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 1rem; overflow-y: auto; }
  header { display: flex; align-items: center; gap: 0.5rem; }
  h2 { margin: 0; font-size: 1.6rem; font-weight: 650; letter-spacing: -0.01em; }
  h3 { margin: 0; font-size: 1.05rem; }
  .back {
    display: grid; place-items: center; width: 2.25rem; height: 2.25rem;
    border: 1px solid var(--border); border-radius: 50%; background: var(--surface);
  }
  .back:hover { background: var(--surface-2); }
  .back svg { width: 1.1rem; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .panel {
    border: 1px solid var(--border); border-radius: var(--radius);
    background: var(--surface); box-shadow: var(--shadow); padding: 1.1rem 1.25rem;
  }
  .row { display: grid; grid-template-columns: 1fr auto; gap: 1.5rem; align-items: center; }
  @media (max-width: 40rem) { .row { grid-template-columns: 1fr; } }
  .what { display: flex; flex-direction: column; gap: 0.25rem; }
  .status { margin: 0; font-weight: 600; color: var(--ok-text); }
  .status.missing { color: var(--warn); }
  .stack { display: grid; gap: 1.1rem; }
  .plain { margin: 0; font-weight: 600; }
  .switch { position: relative; display: inline-block; width: 46px; height: 26px; }
  .switch input { position: absolute; opacity: 0; inset: 0; width: 100%; height: 100%; margin: 0; cursor: pointer; z-index: 1; }
  .track { position: absolute; inset: 0; border-radius: 13px; background: var(--surface-2); border: 1px solid var(--border); transition: background 120ms; }
  .track::after { content: ""; position: absolute; top: 3px; left: 3px; width: 18px; height: 18px; border-radius: 50%; background: var(--muted); transition: transform 120ms, background 120ms; }
  input:checked + .track { background: var(--accent); border-color: transparent; }
  input:checked + .track::after { transform: translateX(20px); background: var(--on-accent); }
  .switch:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 13px; }
  .help { margin: 0; color: var(--muted); font-size: 0.9rem; max-width: 34rem; }
</style>

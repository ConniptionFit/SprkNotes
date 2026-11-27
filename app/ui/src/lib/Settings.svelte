<script lang="ts">
  import type { DownloadProgress, ModelStatus } from "./types";
  import ModelDownload from "./ModelDownload.svelte";
  import ModelSpeed from "./ModelSpeed.svelte";
  import Segmented from "./Segmented.svelte";
  import AssistantSettings from "./AssistantSettings.svelte";
  import AppUpdates from "./AppUpdates.svelte";
  import DevicePicker from "./DevicePicker.svelte";
  import SaveLocation from "./SaveLocation.svelte";
  import Retention from "./Retention.svelte";
  import CustomWords from "./CustomWords.svelte";
  import Managed from "./Managed.svelte";
  import { isLocked, prefs, setPref, type CaptionWho, type CaptionDelay, type MessageTimes, type Theme } from "./prefs.svelte";
  import { comboFromEvent } from "./hotkey";
  import { hotkeyState } from "./hotkeyState.svelte";

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

  // Picking the shortcut: the button listens for the next combination; Escape cancels.
  let capturing = $state(false);
  function key(e: KeyboardEvent) {
    if (capturing) return;
    if (e.key === "Escape") onback();
  }
  function captureKey(e: KeyboardEvent) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      capturing = false;
      return;
    }
    const combo = comboFromEvent(e);
    if (combo) {
      capturing = false;
      setPref("hotkeyKeys", combo);
    }
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
        {#if model.ready}<ModelSpeed {sessionActive} />{/if}
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

  <CustomWords {sessionActive} />

  <SaveLocation {sessionActive} />

  <Retention />

  <DevicePicker source="mic" />
  <DevicePicker source="system" />

  <AppUpdates />

  <AssistantSettings {sessionActive} />

  <div class="panel">
    <div class="row">
      <div class="what">
        <h3>Appearance</h3>
        <p class="help">The overlay is dark by default. Choose Light to make the overlay light too.</p>
      </div>
      <Managed locked={isLocked("theme")}>
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
        disabled={isLocked("theme")}
      />
      </Managed>
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
        <Managed locked={isLocked("captionsOn")}>
        <label class="switch">
          <input
            type="checkbox"
            role="switch"
            checked={prefs.captionsOn}
            disabled={isLocked("captionsOn")}
            onchange={(e) => setPref("captionsOn", e.currentTarget.checked)}
          />
          <span class="track" aria-hidden="true"></span>
          <span class="sr-only">Show live captions on the overlay</span>
        </label>
        </Managed>
      </div>
      <div class="row">
        <div class="what">
          <p class="plain">Live line in the transcript window</p>
          <p class="help">The newest words grow at the end of the transcript before they settle into a line.</p>
        </div>
        <Managed locked={isLocked("liveLine")}>
        <label class="switch">
          <input
            type="checkbox"
            role="switch"
            checked={prefs.liveLine}
            disabled={isLocked("liveLine")}
            onchange={(e) => setPref("liveLine", e.currentTarget.checked)}
          />
          <span class="track" aria-hidden="true"></span>
          <span class="sr-only">Show the live line in the transcript window</span>
        </label>
        </Managed>
      </div>
      <div class="row">
        <div class="what">
          <p class="plain">Message times</p>
          <p class="help">When each line's time shows in the transcript window.</p>
        </div>
        <Segmented
          name="message-times"
          label="When each line's time shows in the transcript window"
          value={prefs.messageTimes}
          options={[
            { value: "hover", text: "On hover" },
            { value: "always", text: "Always" },
          ]}
          onchange={(v: MessageTimes) => setPref("messageTimes", v)}
        />
      </div>
      <div class="row">
        <div class="what">
          <p class="plain">Live text delay</p>
          <p class="help">A short wait lets words settle, so they change less. Steady is under one second.</p>
        </div>
        <Managed locked={isLocked("captionDelay")}>
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
          disabled={isLocked("captionDelay")}
        />
        </Managed>
      </div>
      <div class="row">
        <div class="what">
          <p class="plain">Whose words in the pop-out</p>
          <p class="help">The pop-out's captions show Others only by default. The transcript window always shows both.</p>
        </div>
        <Managed locked={isLocked("captionsWho")}>
        <Segmented
          name="caption-who"
          label="Whose words appear in the pop-out captions"
          value={prefs.captionsWho}
          options={[
            { value: "others", text: "Others only" },
            { value: "both", text: "Others and Me" },
          ]}
          onchange={(v: CaptionWho) => setPref("captionsWho", v)}
          disabled={isLocked("captionsWho")}
        />
        </Managed>
      </div>
    </div>
  </div>

  <div class="panel">
    <div class="stack">
      <div class="row">
        <div class="what">
          <h3>When another app uses the microphone</h3>
          <p class="help">
            While SprkNotes is in the background, offer to start a transcript once a call or another app
            has held the microphone for a few seconds.
          </p>
        </div>
        <Managed locked={isLocked("micPromptOn")}>
        <label class="switch">
          <input
            type="checkbox"
            role="switch"
            checked={prefs.micPromptOn}
            disabled={isLocked("micPromptOn")}
            onchange={(e) => setPref("micPromptOn", e.currentTarget.checked)}
          />
          <span class="track" aria-hidden="true"></span>
          <span class="sr-only">Offer to start when another app uses the microphone</span>
        </label>
        </Managed>
      </div>
      {#if prefs.micPromptSkip.length > 0}
        <div class="what">
          <p class="plain">Not asking for</p>
          <ul class="skipped">
            {#each prefs.micPromptSkip as a (a.id)}
              <li>
                <span>{a.name}</span>
                <button
                  class="small"
                  aria-label={`Ask again for ${a.name}`}
                  onclick={() => setPref("micPromptSkip", prefs.micPromptSkip.filter((s) => s.id !== a.id))}
                >Ask again</button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}
    </div>
  </div>

  <div class="panel">
    <div class="stack">
      <div class="row">
        <div class="what">
          <h3>Keyboard shortcut</h3>
          <p class="help">
            Start a transcript from any app, and pause or resume it, with one key combination. Off until you
            turn it on.
          </p>
        </div>
        <Managed locked={isLocked("hotkeyOn")}>
        <label class="switch">
          <input
            type="checkbox"
            role="switch"
            checked={prefs.hotkeyOn}
            disabled={isLocked("hotkeyOn")}
            onchange={(e) => setPref("hotkeyOn", e.currentTarget.checked)}
          />
          <span class="track" aria-hidden="true"></span>
          <span class="sr-only">Use a keyboard shortcut to start, pause and resume</span>
        </label>
        </Managed>
      </div>
      <div class="row">
        <div class="what">
          <p class="plain">Keys</p>
          <p class="help">
            {#if capturing}Press the new combination, with Ctrl, Alt or Win. Esc cancels.{:else}Click to change.{/if}
          </p>
          {#if prefs.hotkeyOn && hotkeyState.problem}
            <p class="problem" role="alert">{hotkeyState.problem}</p>
          {/if}
        </div>
        <Managed locked={isLocked("hotkeyKeys")}>
        <button
          class="keys"
          disabled={isLocked("hotkeyKeys")}
          class:capturing
          aria-label={capturing ? "Press the new shortcut" : `Shortcut: ${prefs.hotkeyKeys}. Click to change`}
          onclick={() => (capturing = !capturing)}
          onkeydown={captureKey}
          onblur={() => (capturing = false)}
        >{capturing ? "Press keys" : prefs.hotkeyKeys}</button>
        </Managed>
      </div>
    </div>
  </div>

  <div class="panel">
    <div class="stack">
      <h3>Misc</h3>
    <div class="row">
      <div class="what">
        <p class="plain">Throwback Mode</p>
        <p class="help">Get a blast from the past: the original app icon. Everything else stays the same.</p>
      </div>
      <Managed locked={isLocked("throwback")}>
      <label class="switch">
        <input
          type="checkbox"
          role="switch"
          checked={prefs.throwback}
          disabled={isLocked("throwback")}
          onchange={(e) => setPref("throwback", e.currentTarget.checked)}
        />
        <span class="track" aria-hidden="true"></span>
        <span class="sr-only">Turn on Throwback Mode</span>
      </label>
      </Managed>
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
  .switch:has(input:disabled) { opacity: 0.55; }
  .keys:disabled { opacity: 0.55; cursor: not-allowed; }
  .switch:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 13px; }
  .help { margin: 0; color: var(--muted); font-size: 0.9rem; max-width: 34rem; }
  .skipped { list-style: none; margin: 0; padding: 0; display: grid; gap: 0.35rem; }
  .skipped li { display: flex; align-items: center; justify-content: space-between; gap: 1rem; max-width: 34rem; }
  .small {
    padding: 0.25rem 0.75rem; border-radius: 999px; border: 1px solid var(--border);
    background: transparent; color: var(--text); font: inherit; font-size: 0.85rem; cursor: pointer;
  }
  .small:hover { background: var(--surface-2); }
  .small:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .keys {
    min-width: 8.5rem; padding: 0.4rem 0.9rem; border-radius: 8px; border: 1px solid var(--border);
    background: var(--surface-2); color: var(--text); font: inherit; font-weight: 600; cursor: pointer;
    font-variant-numeric: tabular-nums;
  }
  .keys.capturing { border-color: var(--accent); color: var(--accent); }
  .keys:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .problem { margin: 0; color: var(--warn); font-size: 0.9rem; }
</style>

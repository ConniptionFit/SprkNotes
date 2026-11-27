<script lang="ts">
  // The phone app (ADR-015, design D "Conversation"): one screen of speech bubbles with a control
  // bar at the bottom, and Settings behind the gear. Same fonts, colors, icons and sound meter as
  // the desktop app. The phone hears one microphone, so lines carry a time and no speaker label.
  import { onDestroy, onMount } from "svelte";
  import { backend, type Backend } from "../api";
  import { prefs, setPref, type Theme } from "../prefs.svelte";
  import Segmented from "../Segmented.svelte";
  import SourceButton from "../SourceButton.svelte";
  import type { Line, Phase, Status } from "../types";
  import { CLOCK_START, elapsed, hms, hold, run, type Clock } from "./clock";

  let api: Backend | null = null;
  let screen = $state<"live" | "settings">("live");
  let status = $state<Status>("ready");
  let phase = $state<Phase>({ phase: "idle" });
  let lines = $state<Line[]>([]);
  let live = $state("");
  let level = $state(0);
  let name = $state("");
  let notice = $state("");
  let version = $state("");
  let mic = $state(prefs.micOn);
  let clock = $state<Clock>(CLOCK_START);
  let now = $state(Date.now());
  let list = $state<HTMLDivElement>();
  let follow = true;

  const active = $derived(status !== "ready");
  const busy = $derived(phase.phase !== "idle");
  const clockText = $derived(hms(elapsed(clock, now)));
  const loadingText = $derived(
    phase.phase === "loading" ? `Loading ${phase.model ?? "the model"}…` : phase.phase === "ending" ? "Saving the transcript…" : "",
  );

  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  function say(text: string) {
    notice = text;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = ""), 9000);
  }

  function problemText(kind: string): string {
    if (kind === "write_failed") return "A line could not be saved to your transcript. Check that the phone has free space.";
    if (kind === "finalize_failed") return "The transcript could not be closed properly. It still holds everything saved so far.";
    return "Something went wrong with the session.";
  }

  // Keep the newest bubble in view unless the person scrolled up to read.
  function onScroll() {
    const el = list;
    if (el) follow = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
  }
  $effect(() => {
    void [lines.length, live];
    const el = list;
    if (el && follow) queueMicrotask(() => (el.scrollTop = el.scrollHeight));
  });

  const offs: (() => void)[] = [];
  let ticker: ReturnType<typeof setInterval> | undefined;
  onMount(async () => {
    const b = await backend;
    api = b;
    b.setDevice("mic", prefs.micDevice).catch(() => {});
    b.setSource("mic", mic).catch(() => {});
    offs.push(
      await b.onLine((l) => {
        lines = [...lines, l];
        if (l.kind === "statement") live = "";
      }),
    );
    offs.push(await b.onCaption((c) => (live = c.final ? "" : c.text)));
    offs.push(await b.onLevels((l) => (level = l[0] ?? 0)));
    offs.push(
      await b.onStatus((s) => {
        const t = Date.now();
        if (s === "transcribing") clock = run(clock, t);
        else clock = hold(clock, t);
        if (s === "ready") live = "";
        status = s;
      }),
    );
    offs.push(await b.onPhase((p) => (phase = p)));
    offs.push(await b.onProblem((kind) => say(problemText(kind))));
    version = await import("@tauri-apps/api/app").then((m) => m.getVersion()).catch(() => "");
    ticker = setInterval(() => (now = Date.now()), 500);
  });
  onDestroy(() => {
    offs.forEach((off) => off());
    clearInterval(ticker);
    clearTimeout(noticeTimer);
  });

  async function start() {
    lines = [];
    live = "";
    clock = CLOCK_START;
    follow = true;
    phase = { phase: "loading" };
    try {
      await api?.start(name.trim());
    } catch (e) {
      say(String(e));
    }
  }

  async function act(f: (() => Promise<void>) | undefined) {
    try {
      await f?.();
    } catch (e) {
      say(String(e));
    }
  }

  // The name typed before Start titles the new transcript; a change mid-session renames it.
  function onName() {
    if (active) act(() => api!.rename(name.trim()));
  }

  function onSource(_s: "mic" | "system", on: boolean) {
    mic = on;
    setPref("micOn", on);
    act(() => api!.setSource("mic", on));
  }

  const themes: { value: Theme; text: string }[] = [
    { value: "system", text: "System" },
    { value: "light", text: "Light" },
    { value: "dark", text: "Dark" },
  ];
</script>

<div class="phone">
  <header class="top">
    {#if screen === "live"}
      <label for="session-name" class="hidden">Session name</label>
      <input id="session-name" class="name" placeholder="Name this session" bind:value={name} onchange={onName} autocomplete="off" />
    {:else}
      <button class="ib" aria-label="Back" onclick={() => (screen = "live")}>
        <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M12.5 4L6.5 10l6 6" /></svg>
      </button>
      <h1>Settings</h1>
    {/if}
    {#if screen === "live"}
      <button class="ib" aria-label="Settings" onclick={() => (screen = "settings")}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1.08-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1.08 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h0a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h0a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" /></svg>
      </button>
    {/if}
  </header>

  {#if screen === "live"}
    <div class="lines" bind:this={list} onscroll={onScroll} aria-live="polite">
      {#if lines.length === 0 && !live}
        <p class="empty">{loadingText || "Lines appear here as people speak."}</p>
      {/if}
      {#each lines as l (l.id)}
        {#if l.kind === "statement"}
          <div class="msg"><span class="tm">{l.t}</span><p class="bub">{l.text}</p></div>
        {:else if l.kind === "mark"}
          <p class="event"><span class="tm">{l.t}</span> Mark</p>
        {:else if l.kind === "note"}
          <p class="event"><span class="tm">{l.t}</span> Note: {l.text}</p>
        {:else}
          <p class="event"><span class="tm">{l.t}</span> {l.text}</p>
        {/if}
      {/each}
      {#if live}
        <div class="msg partial"><span class="tm now">live</span><p class="bub">{live}</p></div>
      {/if}
    </div>

    {#if notice}<p class="notice" role="status">{notice}</p>{/if}

    <div class="bar">
      <SourceButton source="mic" bind:on={mic} level={status === "transcribing" ? level : null} onsource={onSource} />
      <span class="clock" class:rec={status === "transcribing"}>{active ? clockText : loadingText ? "" : "00:00:00"}</span>
      {#if active}
        <button class="pill ghost" disabled={busy} onclick={() => act(() => api!.end())}>End</button>
      {/if}
      {#if status === "ready"}
        <button class="round" aria-label="Start" disabled={busy} onclick={start}>
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6.5 4l10 6-10 6z" /></svg>
        </button>
      {:else if status === "transcribing"}
        <button class="round" aria-label="Pause" disabled={busy} onclick={() => act(() => api!.pause())}>
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6 4h2.5v12H6zM11.5 4H14v12h-2.5z" /></svg>
        </button>
      {:else}
        <button class="round" aria-label="Resume" disabled={busy} onclick={() => act(() => api!.resume())}>
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M6.5 4l10 6-10 6z" /></svg>
        </button>
      {/if}
    </div>
  {:else}
    <div class="settings">
      <p class="sec">Appearance</p>
      <div class="card">
        <div class="set"><span class="lbl">Theme</span><Segmented name="theme" label="Theme" options={themes} value={prefs.theme} onchange={(v) => setPref("theme", v)} /></div>
      </div>
      <p class="sec">Transcripts</p>
      <div class="card">
        <div class="set">
          <div><div class="lbl">Speaker labels</div><div class="sub">A phone hears one microphone, so lines are not tagged by person.</div></div>
          <span class="val">Off</span>
        </div>
        <div class="set">
          <div><div class="lbl">Saved to</div><div class="sub">Private to SprkNotes. Removed if the app is uninstalled.</div></div>
          <span class="val">This phone</span>
        </div>
      </div>
      {#if version}<p class="about">SprkNotes {version}</p>{/if}
    </div>
  {/if}
</div>

<style>
  /* The app runs edge to edge: keep clear of the status bar and the gesture bar. */
  .phone {
    height: 100dvh;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    color: var(--text);
    padding-top: max(env(safe-area-inset-top, 0px), 24px);
    overflow: hidden;
  }
  .hidden { position: absolute; left: -9999px; }
  .top { display: flex; align-items: center; gap: 4px; padding: 10px 10px 8px 20px; }
  .name, h1 { flex: 1; min-width: 0; margin: 0; font: inherit; font-size: 21px; font-weight: 650; color: var(--text); background: transparent; border: none; padding: 0; }
  .name::placeholder { color: var(--muted); }
  .name:focus-visible { outline: none; }
  .ib { width: 44px; height: 44px; border-radius: 999px; border: none; background: transparent; display: inline-flex; align-items: center; justify-content: center; color: var(--muted); flex: none; }
  .ib svg { width: 22px; height: 22px; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .ib:focus-visible, button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

  .lines { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; padding: 0 16px 12px; overscroll-behavior: contain; }
  .lines > :first-child { margin-top: auto; }
  .empty { margin: auto; text-align: center; color: var(--muted); font-size: 15px; padding: 0 24px; }
  .msg { display: flex; flex-direction: column; align-items: flex-start; }
  .tm { font-size: 11.5px; color: var(--muted); font-variant-numeric: tabular-nums; margin: 8px 6px 3px; }
  .tm.now { color: var(--live); }
  .bub { margin: 0; max-width: 88%; padding: 10px 14px; border-radius: 18px; border-bottom-left-radius: 6px; background: var(--surface-2); font-size: 15.5px; line-height: 1.4; overflow-wrap: anywhere; }
  .partial { opacity: 0.7; }
  .event { margin: 10px 0 2px; text-align: center; font-size: 12.5px; color: var(--muted); }
  .event .tm { margin: 0; }
  .notice { margin: 0 16px 8px; padding: 10px 14px; border-radius: var(--radius); background: var(--warn-soft); color: var(--text); font-size: 14px; }

  .bar { display: flex; align-items: center; gap: 10px; padding: 10px 14px max(env(safe-area-inset-bottom, 0px), 18px); background: var(--surface); border-top: 1px solid var(--border); }
  /* The microphone button is the desktop's, made a touch target; its menu opens upward here. */
  .bar :global(.src) { height: 44px; padding: 0 12px; gap: 7px; }
  .bar :global(.src svg) { width: 18px; height: 18px; }
  .bar :global(.src .caret) { width: 10px; height: 10px; }
  .bar :global(.menu) { top: auto; bottom: calc(100% + 6px); left: 0; right: auto; }
  .bar :global(.menu button) { min-height: 44px; font-size: 15px; }
  .clock { flex: 1; font-size: 15px; font-weight: 600; font-variant-numeric: tabular-nums; color: var(--muted); }
  .clock.rec { color: var(--live); }
  .pill { height: 44px; padding: 0 16px; border-radius: 999px; font: inherit; font-weight: 600; font-size: 16px; }
  .ghost { background: transparent; border: 1px solid var(--border); color: var(--text); }
  .round { width: 52px; height: 52px; border-radius: 50%; border: none; background: var(--accent); color: var(--on-accent); display: inline-flex; align-items: center; justify-content: center; flex: none; }
  .round svg { width: 20px; height: 20px; fill: currentColor; }
  button:disabled { opacity: 0.55; }

  .settings { flex: 1; min-height: 0; overflow-y: auto; padding: 4px 20px max(env(safe-area-inset-bottom, 0px), 20px); }
  .sec { font-size: 13px; font-weight: 700; color: var(--muted); text-transform: uppercase; letter-spacing: 0.04em; margin: 18px 4px 6px; }
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 16px; }
  .set { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 14px; min-height: 56px; border-top: 1px solid var(--border); flex-wrap: wrap; }
  .set:first-child { border-top: none; }
  .lbl { font-size: 15px; }
  .sub { font-size: 12.5px; color: var(--muted); margin-top: 2px; }
  .val { font-size: 14px; color: var(--muted); }
  .about { margin: 20px 4px; font-size: 12.5px; color: var(--muted); }
</style>

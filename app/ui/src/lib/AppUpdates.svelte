<script lang="ts">
  import Managed from "./Managed.svelte";
  import { MANAGED_TEXT } from "./managed";
  import { onMount } from "svelte";
  import { backend, type Backend } from "./api";
  import type { AppUpdateProgress, AppUpdateStatus } from "./types";
  import Segmented from "./Segmented.svelte";
  import { progressText, progressValue } from "./appUpdate";

  let api = $state.raw<Backend | undefined>();
  let st = $state<AppUpdateStatus | null>(null);
  let message = $state("");
  let working = $state(false);
  let found = $state<string | null>(null);
  let progress = $state<AppUpdateProgress | null>(null);

  async function refresh() {
    try {
      st = (await api?.appUpdateStatus()) ?? null;
    } catch {
      st = null;
    }
  }

  onMount(() => {
    let stop: (() => void) | undefined;
    backend.then(async (b) => {
      api = b;
      stop = await b.onAppUpdateProgress((p) => (progress = p));
      await refresh();
    });
    return () => stop?.();
  });

  async function change(c: { channel?: string; mode?: string }) {
    message = "";
    try {
      await api?.appUpdateSet(c);
      found = null;
    } catch (e) {
      message = String(e);
    }
    await refresh();
  }

  async function check() {
    working = true;
    message = "";
    try {
      const r = await api?.appUpdateCheck();
      found = r?.available ? (r.version ?? "newer") : null;
      message = r?.available ? "" : "You have the latest version.";
    } catch (e) {
      message = String(e);
    } finally {
      working = false;
    }
  }

  async function install() {
    working = true;
    message = "";
    try {
      const v = await api?.appUpdateInstall();
      message = v
        ? "The update is ready. It installs when you close SprkNotes, with a progress window."
        : "You have the latest version.";
      found = null;
    } catch (e) {
      message = String(e);
    } finally {
      working = false;
      await refresh();
    }
  }

  async function updateNow() {
    working = true;
    message = "";
    progress = null;
    try {
      const v = await api?.appUpdateNow();
      if (!v) {
        progress = null;
        message = "You have the latest version.";
        found = null;
      }
    } catch (e) {
      progress = null;
      message = String(e);
      working = false;
      await refresh();
    }
  }

  const bar = $derived(progressValue(progress));
  const locked = $derived(!!st && (st.channelLocked || st.modeLocked));
</script>

{#if st && st.mode !== "off" || (st && st.modeLocked)}
  <div class="panel">
    <div class="stack">
      <div class="row">
        <div class="what">
          <h3>Updates</h3>
          <p class="help">
            Version {st.installed}.
            {#if st.managedInstall}Your IT team keeps this PC up to date in the background.{/if}
            {#if st.mode === "off"}Updates are turned off by your organization.{/if}
          </p>
        </div>
        {#if st.mode !== "off" && !st.managedInstall}
          <button class="btn" onclick={check} disabled={working || st.busy}>
            {working ? "Checking…" : "Check for updates"}
          </button>
        {/if}
      </div>

      {#if st.mode !== "off"}
        <div class="row">
          <div class="what">
            <p class="plain">Release channel</p>
            <p class="help">
              {#if st.channelLocked}{MANAGED_TEXT}{:else}Stable is best for most people. Beta and Alpha get new features earlier and may have rough edges.{/if}
            </p>
          </div>
          {#if st.channelLocked}
            <Managed locked={true}><span class="chip">{st.channel}</span></Managed>
          {:else}
            <Segmented
              name="channel"
              label="Release channel"
              value={st.channel}
              options={[
                { value: "stable", text: "Stable" },
                { value: "beta", text: "Beta" },
                { value: "alpha", text: "Alpha" },
              ]}
              onchange={(v: string) => change({ channel: v })}
            />
          {/if}
        </div>
      {/if}

      {#if (found || st.readyToInstall) && !st.managedInstall && st.mode !== "off"}
        <div class="row">
          <div class="what">
            <p class="plain">{found ? `Version ${found} is available.` : "An update is ready."}</p>
            <p class="help">
              Update now closes SprkNotes, installs and opens it again.
              {#if st.readyToInstall}Otherwise it installs when you close SprkNotes.{/if}
            </p>
          </div>
          <div class="actions">
            {#if found && !st.readyToInstall}
              <button class="btn" onclick={install} disabled={working}>Update when I close</button>
            {/if}
            <button class="btn primary" onclick={updateNow} disabled={working}>Update now</button>
          </div>
        </div>
      {/if}
      {#if progress}
        <div class="progress" role="status">
          <div
            class="track"
            role="progressbar"
            aria-label="Update progress"
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={bar ?? undefined}
          >
            <div class="fill" class:busy={bar == null} style:width={bar == null ? "35%" : `${bar}%`}></div>
          </div>
          <p class="help">{progressText(progress)}</p>
        </div>
      {/if}
      {#if message}<p class="help" role="status">{message}</p>{/if}
      {#if locked}<p class="help">Some update settings are managed by your administrator.</p>{/if}
    </div>
  </div>
{/if}

<style>
  .panel { border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); box-shadow: var(--shadow); padding: 1.1rem 1.25rem; }
  .stack { display: grid; gap: 1.1rem; }
  .row { display: grid; grid-template-columns: 1fr auto; gap: 1.5rem; align-items: center; }
  @media (max-width: 40rem) { .row { grid-template-columns: 1fr; } }
  .what { display: flex; flex-direction: column; gap: 0.25rem; }
  h3 { margin: 0; font-size: 1.05rem; }
  .plain { margin: 0; font-weight: 600; }
  .help { margin: 0; color: var(--muted); font-size: 0.9rem; max-width: 34rem; }
  .btn { padding: 0.5rem 1rem; border: 1px solid var(--border); border-radius: 999px; background: var(--surface-2); font-weight: 600; cursor: pointer; }
  .btn.primary { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .btn:disabled { opacity: 0.6; cursor: default; }
  .actions { display: flex; gap: 0.5rem; flex-wrap: wrap; }
  .progress { display: grid; gap: 0.4rem; }
  .track { height: 0.45rem; border-radius: 999px; background: var(--surface-2); border: 1px solid var(--border); overflow: hidden; }
  .fill { height: 100%; background: var(--accent); transition: width 0.3s ease; }
  .fill.busy { animation: slide 1.2s ease-in-out infinite alternate; }
  @keyframes slide { from { margin-left: 0; } to { margin-left: 65%; } }
  @media (prefers-reduced-motion: reduce) { .fill, .fill.busy { transition: none; animation: none; } }
  .chip { padding: 0.35rem 0.9rem; border-radius: 999px; background: var(--surface-2); border: 1px solid var(--border); font-weight: 600; text-transform: capitalize; }
</style>

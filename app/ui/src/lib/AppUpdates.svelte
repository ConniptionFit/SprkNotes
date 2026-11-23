<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type Backend } from "./api";
  import type { AppUpdateStatus } from "./types";
  import Segmented from "./Segmented.svelte";

  let api = $state.raw<Backend | undefined>();
  let st = $state<AppUpdateStatus | null>(null);
  let message = $state("");
  let working = $state(false);
  let found = $state<string | null>(null);

  async function refresh() {
    try {
      st = (await api?.appUpdateStatus()) ?? null;
    } catch {
      st = null;
    }
  }

  onMount(() => {
    backend.then(async (b) => {
      api = b;
      await refresh();
    });
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
      message = v ? "The update is ready. It installs when you close SprkNotes." : "You have the latest version.";
      found = null;
    } catch (e) {
      message = String(e);
    } finally {
      working = false;
      await refresh();
    }
  }

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
              {#if st.channelLocked}Managed by your organization.{:else}Stable is best for most people. Beta and Alpha get new features earlier and may have rough edges.{/if}
            </p>
          </div>
          {#if st.channelLocked}
            <span class="chip">{st.channel}</span>
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

      {#if found && !st.managedInstall}
        <div class="row">
          <p class="plain">Version {found} is available.</p>
          <button class="btn primary" onclick={install} disabled={working}>Update when I close the app</button>
        </div>
      {/if}
      {#if st.readyToInstall}
        <p class="help">An update is ready and installs when you close SprkNotes.</p>
      {/if}
      {#if message}<p class="help" role="status">{message}</p>{/if}
      {#if locked}<p class="help">Some update settings are managed by your organization.</p>{/if}
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
  .chip { padding: 0.35rem 0.9rem; border-radius: 999px; background: var(--surface-2); border: 1px solid var(--border); font-weight: 600; text-transform: capitalize; }
</style>

<script lang="ts">
  import type { Backend } from "./api";
  import type { ExportFormat, LibraryEntry, TranscriptView } from "./types";

  let {
    api,
    onback,
    onsummarize,
    notice = "",
  }: {
    api: Backend | undefined;
    onback: () => void;
    /** Summarize this transcript (the app picks Claude or Copilot, or asks). */
    onsummarize: (path: string) => void;
    notice?: string;
  } = $props();

  let query = $state("");
  let entries = $state<LibraryEntry[]>([]);
  let loaded = $state(false);
  let open = $state<{ entry: LibraryEntry; view: TranscriptView } | null>(null);
  let message = $state("");
  let exportMenu = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function refresh() {
    try {
      entries = (await api?.libraryList(query)) ?? [];
    } catch (e) {
      message = String(e);
    }
    loaded = true;
  }

  // Load when the backend is ready, and again (after a short pause) as the search text changes.
  $effect(() => {
    void api;
    void query;
    clearTimeout(timer);
    timer = setTimeout(refresh, loaded ? 200 : 0);
    return () => clearTimeout(timer);
  });

  async function show(entry: LibraryEntry) {
    message = "";
    try {
      const view = await api?.libraryOpen(entry.path);
      if (view) open = { entry, view };
    } catch (e) {
      message = String(e);
    }
  }

  async function doExport(f: ExportFormat) {
    exportMenu = false;
    if (!open) return;
    try {
      const name = await api?.libraryExport(open.entry.path, f);
      message = `Saved ${name} next to the transcript.`;
    } catch (e) {
      message = String(e);
    }
  }

  function key(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (exportMenu) exportMenu = false;
    else if (open) open = null;
    else onback();
  }

  const when = (e: LibraryEntry) => [e.date, e.start && e.start.length <= 5 ? e.start : ""].filter(Boolean).join(" ");
</script>

<svelte:window onkeydown={key} />

<section class="library" aria-labelledby="library-title">
  <header>
    <button class="back" onclick={() => (open ? (open = null) : onback())} aria-label={open ? "Back to the list" : "Back to transcript"}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M12 4l-6 6 6 6" /></svg>
    </button>
    <h2 id="library-title">{open ? open.view.title || open.entry.name : "Library"}</h2>
  </header>

  {#if open}
    <div class="actions">
      <button onclick={() => api?.libraryReveal(open!.entry.path)}>View in folder</button>
      <div class="menu">
        <button aria-haspopup="menu" aria-expanded={exportMenu} onclick={() => (exportMenu = !exportMenu)}>Export</button>
        {#if exportMenu}
          <div class="pop" role="menu">
            <button role="menuitem" onclick={() => doExport("docx")}>Word document</button>
            <button role="menuitem" onclick={() => doExport("txt")}>Plain text</button>
            <button role="menuitem" onclick={() => doExport("json")}>JSON</button>
          </div>
        {/if}
      </div>
      <button class="primary" onclick={() => onsummarize(open!.entry.path)}>Summarize</button>
    </div>
    <p class="meta">{when(open.entry)}{open.view.duration ? ` · ${open.view.duration}` : ""}</p>
    {#if message || notice}<p class="note" role="status">{message || notice}</p>{/if}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="body" tabindex="0" role="region" aria-label="Transcript">
      {#each open.view.lines as l, i (i)}
        {#if l.kind === "statement"}
          <p class="line" class:me={l.speaker === "Me"} class:others={l.speaker !== "Me"}>
            <time>{l.t}</time><strong>{l.speaker}</strong><span class="text">{l.text}</span>
          </p>
        {:else}
          <p class="line event"><time>{l.t}</time><span class="text">{l.text}</span></p>
        {/if}
      {:else}
        <p class="empty">This transcript has no lines yet.</p>
      {/each}
    </div>
  {:else}
    <label class="search">
      <span class="sr-only">Search transcripts</span>
      <input type="search" placeholder="Search names and words" bind:value={query} />
    </label>
    {#if message || notice}<p class="note" role="status">{message || notice}</p>{/if}
    <ul class="list">
      {#each entries as e (e.path)}
        <li>
          <button class="item" onclick={() => show(e)}>
            <span class="title">{e.title || e.name}</span>
            <span class="sub">{when(e)}{e.duration ? ` · ${e.duration}` : ""}{e.folder ? ` · ${e.folder}` : ""}{e.status === "interrupted" ? " · ended unexpectedly" : ""}</span>
          </button>
        </li>
      {/each}
    </ul>
    {#if loaded && entries.length === 0}
      <p class="empty">{query.trim() ? "No transcript matches that search." : "Your transcripts will appear here after your first session."}</p>
    {/if}
  {/if}
</section>

<style>
  .library { flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 0.9rem; }
  header { display: flex; align-items: center; gap: 0.5rem; }
  h2 { margin: 0; font-size: 1.6rem; font-weight: 650; letter-spacing: -0.01em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .back { display: grid; place-items: center; width: 2.25rem; height: 2.25rem; flex: none; border: 1px solid var(--border); border-radius: 50%; background: var(--surface); }
  .back:hover { background: var(--surface-2); }
  .back svg { width: 1.1rem; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .search input { width: 100%; box-sizing: border-box; padding: 0.6rem 0.9rem; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); color: var(--text); font: inherit; }
  .list { list-style: none; margin: 0; padding: 0; overflow-y: auto; display: grid; gap: 0.4rem; min-height: 0; }
  .item { width: 100%; text-align: left; display: grid; gap: 0.15rem; padding: 0.7rem 0.9rem; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); color: var(--text); }
  .item:hover { background: var(--surface-2); }
  .title { font-weight: 600; }
  .sub, .meta, .note, .empty { color: var(--muted); font-size: 0.9rem; margin: 0; }
  .note { color: var(--ok-text); }
  .empty { text-align: center; padding: 2rem 0; }
  .actions { display: flex; gap: 0.5rem; flex-wrap: wrap; }
  .actions button { padding: 0.45rem 0.9rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); }
  .actions button:hover { background: var(--surface-2); }
  .actions .primary { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .actions .primary:hover { background: var(--accent-hover); }
  .menu { position: relative; }
  .pop { position: absolute; z-index: 5; top: calc(100% + 4px); left: 0; display: grid; min-width: 11rem; padding: 0.3rem; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); box-shadow: var(--shadow); }
  .pop button { border: 0; text-align: left; }
  .body { flex: 1; min-height: 0; overflow-y: auto; padding: 0.25rem 0; }
  .line { display: grid; grid-template-columns: 4.5rem 5rem 1fr; gap: 0.6rem; align-items: baseline; margin: 0; padding: 0.35rem 0.5rem; border-radius: 8px; }
  .line:hover { background: var(--surface-2); }
  @media (max-width: 40rem) { .line { grid-template-columns: auto 1fr; } .text { grid-column: 1 / -1; } }
  time { color: var(--muted); font-variant-numeric: tabular-nums; font-size: 0.8em; }
  strong { font-size: 0.85em; font-weight: 650; }
  .me strong { color: var(--me); }
  .others strong { color: var(--others); }
  .event { color: var(--muted); font-style: italic; }
  .event .text { grid-column: 2 / -1; }
</style>

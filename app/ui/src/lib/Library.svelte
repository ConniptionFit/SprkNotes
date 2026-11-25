<script lang="ts">
  import type { Backend } from "./api";
  import type { ExportFormat, LibraryEntry, TranscriptView } from "./types";
  import { ALL, renamed, shows, tree, type FolderFilter } from "./folders";

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
  let moveMenu = $state(false);
  let folders = $state<string[]>([]);
  let filter = $state<FolderFilter>(ALL);
  /** The folder name form under the list: a new folder, or renaming the selected one. */
  let folderForm = $state<null | "new" | "rename">(null);
  let folderName = $state("");
  /** Inline rename of the open transcript, and the Delete question. */
  let renaming = $state(false);
  let newTitle = $state("");
  let confirmDelete = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const shown = $derived(entries.filter((e) => shows(e, filter)));
  const folderTree = $derived(tree(folders));
  const selected = $derived(filter.kind === "folder" && filter.path !== "" ? filter.path : "");

  async function refresh() {
    try {
      entries = (await api?.libraryList(query)) ?? [];
      folders = (await api?.libraryFolders()) ?? [];
    } catch (e) {
      message = String(e);
    }
    loaded = true;
  }

  function filterValue(f: FolderFilter) {
    return f.kind === "all" ? "*" : f.path === "" ? "/" : `/${f.path}`;
  }

  function pickFilter(v: string) {
    filter = v === "*" ? ALL : { kind: "folder", path: v === "/" ? "" : v.slice(1) };
    folderForm = null;
  }

  function startFolder(kind: "new" | "rename") {
    message = "";
    folderForm = kind;
    folderName = kind === "rename" ? (selected.split("/").pop() ?? "") : "";
  }

  async function saveFolder(ev: SubmitEvent) {
    ev.preventDefault();
    try {
      if (folderForm === "new") {
        const made = await api?.folderCreate(selected, folderName);
        if (made !== undefined) filter = { kind: "folder", path: made };
      } else if (folderForm === "rename" && selected) {
        const to = await api?.folderRename(selected, folderName);
        if (to !== undefined) filter = { kind: "folder", path: renamed(selected, selected, to) };
      }
      folderForm = null;
      await refresh();
    } catch (e) {
      message = String(e);
    }
  }

  function startRename() {
    if (!open) return;
    message = "";
    confirmDelete = false;
    newTitle = open.view.title || open.entry.title || open.entry.name;
    renaming = true;
  }

  async function saveRename(ev: SubmitEvent) {
    ev.preventDefault();
    if (!open) return;
    try {
      const path = await api?.libraryRename(open.entry.path, newTitle);
      if (path) {
        const title = newTitle.trim().replace(/\s+/g, " ");
        open = { entry: { ...open.entry, path, title }, view: { ...open.view, title } };
      }
      renaming = false;
      await refresh();
    } catch (e) {
      message = String(e);
    }
  }

  async function moveTo(folder: string) {
    moveMenu = false;
    if (!open) return;
    try {
      const path = await api?.libraryMove(open.entry.path, folder);
      if (path) open = { ...open, entry: { ...open.entry, path, folder } };
      message = folder ? `Moved to ${folder}.` : "Moved to the top of the library.";
      await refresh();
    } catch (e) {
      message = String(e);
    }
  }

  async function doDelete() {
    if (!open) return;
    try {
      await api?.libraryDelete(open.entry.path);
      confirmDelete = false;
      open = null;
      message = "Moved to the Recycle Bin.";
      await refresh();
    } catch (e) {
      message = String(e);
    }
  }

  function close() {
    open = null;
    renaming = false;
    confirmDelete = false;
    moveMenu = false;
    exportMenu = false;
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
      if (view) {
        close();
        open = { entry, view };
      }
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
    else if (moveMenu) moveMenu = false;
    else if (renaming) renaming = false;
    else if (confirmDelete) confirmDelete = false;
    else if (folderForm) folderForm = null;
    else if (open) close();
    else onback();
  }

  const when = (e: LibraryEntry) => [e.date, e.start && e.start.length <= 5 ? e.start : ""].filter(Boolean).join(" ");
</script>

<svelte:window onkeydown={key} />

<section class="library" aria-labelledby="library-title">
  <header>
    <button class="back" onclick={() => (open ? close() : onback())} aria-label={open ? "Back to the list" : "Back to transcript"}>
      <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M12 4l-6 6 6 6" /></svg>
    </button>
    {#if open && renaming}
      <form class="rename" onsubmit={saveRename}>
        <label class="sr-only" for="new-title">Transcript name</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="new-title" bind:value={newTitle} maxlength="120" autofocus />
        <button type="submit" class="primary">Save</button>
        <button type="button" onclick={() => (renaming = false)}>Cancel</button>
      </form>
    {:else}
      <h2 id="library-title">{open ? open.view.title || open.entry.name : "Library"}</h2>
    {/if}
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
      <div class="menu">
        <button aria-haspopup="menu" aria-expanded={moveMenu} onclick={() => ((moveMenu = !moveMenu), (exportMenu = false))}>Move</button>
        {#if moveMenu}
          <div class="pop" role="menu">
            <button role="menuitem" disabled={open.entry.folder === ""} onclick={() => moveTo("")}>Top of the library</button>
            {#each folderTree as f (f.path)}
              <button role="menuitem" disabled={open.entry.folder === f.path} style:padding-left={`${0.6 + f.depth}rem`} onclick={() => moveTo(f.path)}>{f.name}</button>
            {/each}
          </div>
        {/if}
      </div>
      <button onclick={startRename}>Rename</button>
      <button onclick={() => ((confirmDelete = true), (renaming = false))}>Delete</button>
      <button class="primary" onclick={() => onsummarize(open!.entry.path)}>Summarize</button>
    </div>
    {#if confirmDelete}
      <div class="confirm" role="alertdialog" aria-labelledby="delete-q">
        <p id="delete-q">Move this transcript to the Recycle Bin? You can restore it from there.</p>
        <button class="danger" onclick={doDelete}>Move to Recycle Bin</button>
        <button onclick={() => (confirmDelete = false)}>Cancel</button>
      </div>
    {/if}
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
    <div class="folders">
      <label>
        <span class="sr-only">Folder</span>
        <select value={filterValue(filter)} onchange={(e) => pickFilter(e.currentTarget.value)}>
          <option value="*">All transcripts</option>
          <option value="/">Top of the library</option>
          {#each folderTree as f (f.path)}
            <option value={`/${f.path}`}>{"  ".repeat(f.depth + 1)}{f.name}</option>
          {/each}
        </select>
      </label>
      <button onclick={() => startFolder("new")}>New folder</button>
      {#if selected}<button onclick={() => startFolder("rename")}>Rename folder</button>{/if}
    </div>
    {#if folderForm}
      <form class="rename" onsubmit={saveFolder}>
        <label class="sr-only" for="folder-name">{folderForm === "new" ? "New folder name" : "Folder name"}</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="folder-name" bind:value={folderName} maxlength="80" autofocus placeholder={folderForm === "new" ? (selected ? `New folder in ${selected}` : "New folder") : ""} />
        <button type="submit" class="primary">{folderForm === "new" ? "Create" : "Save"}</button>
        <button type="button" onclick={() => (folderForm = null)}>Cancel</button>
      </form>
    {/if}
    {#if message || notice}<p class="note" role="status">{message || notice}</p>{/if}
    <ul class="list">
      {#each shown as e (e.path)}
        <li>
          <button class="item" onclick={() => show(e)}>
            <span class="title">{e.title || e.name}</span>
            <span class="sub">{when(e)}{e.duration ? ` · ${e.duration}` : ""}{e.folder ? ` · ${e.folder}` : ""}{e.status === "interrupted" ? " · ended unexpectedly" : ""}</span>
          </button>
        </li>
      {/each}
    </ul>
    {#if loaded && shown.length === 0}
      <p class="empty">{query.trim() ? "No transcript matches that search." : entries.length ? "This folder has no transcripts yet." : "Your transcripts will appear here after your first session."}</p>
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
  .pop button:disabled { color: var(--muted); }
  .folders, .rename, .confirm { display: flex; gap: 0.5rem; align-items: center; flex-wrap: wrap; }
  .rename { flex: 1; min-width: 0; }
  .folders select, .rename input { padding: 0.45rem 0.7rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); font: inherit; }
  .rename input { flex: 1; min-width: 8rem; }
  .folders button, .rename button, .confirm button { padding: 0.45rem 0.9rem; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); }
  .folders button:hover, .rename button:hover, .confirm button:hover { background: var(--surface-2); }
  .rename .primary { background: var(--accent); color: var(--on-accent); border-color: transparent; }
  .confirm { padding: 0.6rem 0.8rem; border: 1px solid var(--border); border-radius: 10px; background: var(--surface-2); }
  .confirm p { margin: 0; flex: 1 1 16rem; }
  .confirm .danger { color: var(--error-text); border-color: var(--error-text); font-weight: 600; }
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

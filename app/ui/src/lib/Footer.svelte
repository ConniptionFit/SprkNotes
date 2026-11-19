<script lang="ts"></script>

<nav aria-label="More">
  <button disabled title="Arrives with the library">
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 4.5h12M4 10h12M4 15.5h8" /></svg>
    Library
  </button>
  <button disabled title="Arrives with settings">
    <svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="2.5" /><path d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4" /></svg>
    Settings
  </button>
  <button disabled title="Available after a session ends">
    <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M10 3l1.6 4.4L16 9l-4.4 1.6L10 15l-1.6-4.4L4 9l4.4-1.6z" /></svg>
    Summarize
  </button>
</nav>

<style>
  nav { display: flex; flex-wrap: wrap; gap: 0.25rem; }
  button {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.5rem 0.75rem;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
  }
  button:disabled { opacity: 0.6; }
  svg { width: 1.1rem; height: 1.1rem; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
</style>

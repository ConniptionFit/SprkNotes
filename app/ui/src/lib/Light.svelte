<script lang="ts">
  import type { Status } from "./types";
  let { status, size = 14 }: { status: Status; size?: number } = $props();

  // Text-free on purpose: red = live (transcribing), yellow = paused. Callers hide it when idle.
  const label: { [K in Status]: string } = {
    ready: "SprkNotes is ready",
    transcribing: "SprkNotes is transcribing",
    paused: "Transcription is paused",
  };
</script>

<span
  class="light {status}"
  role="status"
  aria-label={label[status]}
  title={label[status]}
  style:--size="{size}px"
></span>

<style>
  /* Status is never colour alone: ready = ring, transcribing = solid dot with a halo,
     paused = two bars. */
  .light {
    position: relative;
    flex: none;
    display: inline-block;
    width: var(--size);
    height: var(--size);
    border-radius: 50%;
    background: var(--light);
    box-shadow: 0 0 0 calc(var(--size) * 0.28) color-mix(in srgb, var(--light) 22%, transparent);
  }
  .ready { --light: var(--ok); background: transparent; border: calc(var(--size) * 0.22) solid var(--light); }
  .transcribing { --light: var(--live); animation: glow 1.8s ease-in-out infinite; }
  .paused { --light: var(--caution); }
  .paused::before,
  .paused::after {
    content: "";
    position: absolute;
    top: 24%;
    bottom: 24%;
    width: 16%;
    border-radius: 1px;
    background: var(--on-light);
  }
  .paused::before { left: 34%; }
  .paused::after { right: 34%; }
  @keyframes glow { 50% { box-shadow: 0 0 0 calc(var(--size) * 0.6) color-mix(in srgb, var(--light) 8%, transparent); } }

  @media (forced-colors: active) {
    .light { forced-color-adjust: none; background: Highlight; border: 0; box-shadow: none; }
    .ready { background: transparent; border: 3px solid Highlight; }
    .paused::before, .paused::after { background: HighlightText; }
  }
</style>

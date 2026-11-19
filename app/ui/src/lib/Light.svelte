<script lang="ts">
  import type { Status } from "./types";
  let { status, size = 14 }: { status: Status; size?: number } = $props();

  // Text-free on purpose: green = ready, red = live (transcribing), yellow = paused.
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
  .light {
    flex: none;
    display: inline-block;
    width: var(--size);
    height: var(--size);
    border-radius: 50%;
    background: var(--light);
    box-shadow: 0 0 0 calc(var(--size) * 0.28) color-mix(in srgb, var(--light) 22%, transparent);
  }
  .ready { --light: var(--ok); }
  .transcribing { --light: var(--live); animation: glow 1.8s ease-in-out infinite; }
  .paused { --light: var(--caution); }
  @keyframes glow { 50% { box-shadow: 0 0 0 calc(var(--size) * 0.6) color-mix(in srgb, var(--light) 8%, transparent); } }
</style>

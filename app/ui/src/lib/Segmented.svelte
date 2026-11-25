<script lang="ts" generics="T extends string">
  let {
    name,
    label,
    options,
    value,
    onchange,
    disabled = false,
  }: {
    name: string;
    label: string;
    options: { value: T; text: string }[];
    value: T;
    onchange: (v: T) => void;
    disabled?: boolean;
  } = $props();
</script>

<div class="seg" class:disabled role="radiogroup" aria-label={label} aria-disabled={disabled}>
  {#each options as o (o.value)}
    <label class:on={value === o.value}>
      <input type="radio" {name} value={o.value} checked={value === o.value} {disabled} onchange={() => onchange(o.value)} />
      {o.text}
    </label>
  {/each}
</div>

<style>
  .seg { display: inline-flex; padding: 3px; border: 1px solid var(--border); border-radius: 999px; background: var(--surface-2); }
  label { position: relative; padding: 0.4rem 1rem; border-radius: 999px; color: var(--muted); font-weight: 600; font-size: 0.9rem; cursor: pointer; }
  label.on { background: var(--accent); color: var(--on-accent); }
  input { position: absolute; opacity: 0; inset: 0; margin: 0; cursor: pointer; }
  .disabled { opacity: 0.55; }
  .disabled label, .disabled input { cursor: not-allowed; }
  label:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  @media (forced-colors: active) { label.on { border: 2px solid Highlight; } }
</style>

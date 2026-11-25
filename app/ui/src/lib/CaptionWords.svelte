<script lang="ts">
  // The words of the line still being spoken in the overlay. Each word is its own span so new
  // words can fade in one after another. Svelte drops whitespace at the start and end of a tag,
  // so the space between words is an explicit {" "} expression; a literal trailing space inside
  // the span vanished and ran the words together ("She'llpullyourcorn"). Styles live in Mini.
  let { words, stable, delay }: { words: string[]; stable: number; delay: (i: number) => number } = $props();
</script>

{#each words as w, i (i)}<span class="w" class:tent={i >= stable} style="--d: {delay(i)}ms">{w}{" "}</span>{/each}


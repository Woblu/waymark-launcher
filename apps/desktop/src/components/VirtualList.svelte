<script lang="ts" generics="T">
  // Renders only the rows on screen, so a list of a thousand mods or thousands
  // of log lines scrolls as smoothly as a list of ten.
  import type { Snippet } from 'svelte';

  let {
    items,
    rowHeight,
    row,
    stickToEnd = false,
  }: { items: T[]; rowHeight: number | ((item: T) => number); row: Snippet<[T, number]>; stickToEnd?: boolean } = $props();

  const OVERSCAN = 8;

  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  let following = true;

  /** offsets[i] is where row i starts, and the last entry is the total height. */
  const offsets = $derived.by(() => {
    const out = new Float64Array(items.length + 1);
    for (let i = 0; i < items.length; i++) {
      out[i + 1] = out[i] + (typeof rowHeight === 'number' ? rowHeight : rowHeight(items[i]));
    }
    return out;
  });

  function rowAt(y: number): number {
    let low = 0;
    let high = items.length;
    while (low < high) {
      const middle = (low + high) >> 1;
      if (offsets[middle + 1] <= y) low = middle + 1;
      else high = middle;
    }
    return low;
  }

  const first = $derived(Math.max(0, rowAt(scrollTop) - OVERSCAN));
  const last = $derived(Math.min(items.length, rowAt(scrollTop + height) + 1 + OVERSCAN));
  const visible = $derived(items.slice(first, last));

  function onscroll() {
    if (!viewport) return;
    scrollTop = viewport.scrollTop;
    following = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 40;
  }

  $effect(() => {
    items.length;
    if (stickToEnd && following && viewport) viewport.scrollTop = viewport.scrollHeight;
  });
</script>

<div class="viewport" bind:this={viewport} bind:clientHeight={height} {onscroll}>
  <div class="spacer" style:height="{offsets[items.length]}px">
    <div class="window" style:transform="translateY({offsets[first]}px)">
      {#each visible as item, index (first + index)}
        <div style:height="{offsets[first + index + 1] - offsets[first + index]}px">{@render row(item, first + index)}</div>
      {/each}
    </div>
  </div>
</div>

<style>
  .viewport { height: 100%; overflow-y: auto; contain: strict; scrollbar-gutter: stable; }
  .spacer { position: relative; }
  .window { will-change: transform; }
</style>

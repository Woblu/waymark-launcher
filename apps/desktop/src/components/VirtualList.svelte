<script lang="ts" generics="T">
  // Renders only the rows on screen, so a list of a thousand mods or thousands
  // of log lines scrolls as smoothly as a list of ten.
  import type { Snippet } from 'svelte';

  let {
    items,
    rowHeight,
    row,
    stickToEnd = false,
    tick = 0,
  }: {
    items: T[];
    rowHeight: number | ((item: T) => number);
    row: Snippet<[T, number]>;
    stickToEnd?: boolean;
    /** Bumped when `items` grew in place, like a log, so it needn't be copied to be noticed. */
    tick?: number;
  } = $props();

  const OVERSCAN = 8;

  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  let following = true;

  const count = $derived.by(() => {
    void tick;
    return items.length;
  });
  /** Rows that are all one height need no table of where each one starts. */
  const fixed = $derived(typeof rowHeight === 'number' ? rowHeight : null);

  /** offsets[i] is where row i starts, and the last entry is the total height. */
  const offsets = $derived.by(() => {
    if (fixed !== null || typeof rowHeight === 'number') return null;
    const out = new Float64Array(count + 1);
    for (let i = 0; i < count; i++) out[i + 1] = out[i] + rowHeight(items[i]);
    return out;
  });

  function start(index: number): number {
    return offsets ? offsets[index] : index * (fixed ?? 0);
  }

  function rowAt(y: number): number {
    if (!offsets) return fixed ? Math.min(count, Math.max(0, Math.floor(y / fixed))) : 0;
    let low = 0;
    let high = count;
    while (low < high) {
      const middle = (low + high) >> 1;
      if (offsets[middle + 1] <= y) low = middle + 1;
      else high = middle;
    }
    return low;
  }

  const first = $derived(Math.max(0, rowAt(scrollTop) - OVERSCAN));
  const last = $derived(Math.min(count, rowAt(scrollTop + height) + 1 + OVERSCAN));
  const visible = $derived.by(() => {
    void tick;
    return items.slice(first, last);
  });

  function onscroll() {
    if (!viewport) return;
    scrollTop = viewport.scrollTop;
    following = viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < 40;
  }

  $effect(() => {
    void count;
    if (stickToEnd && following && viewport) viewport.scrollTop = viewport.scrollHeight;
  });
</script>

<div class="viewport" bind:this={viewport} bind:clientHeight={height} {onscroll}>
  <div class="spacer" style:height="{start(count)}px">
    <div class="window" style:transform="translateY({start(first)}px)">
      {#each visible as item, index (first + index)}
        <div style:height="{start(first + index + 1) - start(first + index)}px">{@render row(item, first + index)}</div>
      {/each}
    </div>
  </div>
</div>

<style>
  .viewport { height: 100%; overflow-y: auto; contain: strict; scrollbar-gutter: stable; }
  .spacer { position: relative; }
  .window { will-change: transform; }
</style>

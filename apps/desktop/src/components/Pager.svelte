<script lang="ts">
  import { ChevronLeft, ChevronRight } from '@lucide/svelte';

  let { page, pages, onpage }: { page: number; pages: number; onpage: (page: number) => void } = $props();

  /** The first page, the last, and the ones either side of this one. */
  const shown = $derived.by(() => {
    const out: (number | null)[] = [];
    for (let number = 1; number <= pages; number++) {
      if (number === 1 || number === pages || Math.abs(number - page) <= 1) out.push(number);
      else if (out.at(-1) !== null) out.push(null);
    }
    return out;
  });
</script>

{#if pages > 1}
  <nav class="pager" aria-label="Pages">
    <button aria-label="Previous page" title="Previous page" disabled={page <= 1} onclick={() => onpage(page - 1)}><ChevronLeft size={17} /></button>
    {#each shown as number, index (number ?? `gap-${index}`)}
      {#if number === null}
        <span class="gap">...</span>
      {:else}
        <button class="tabular" class:active={number === page} aria-label="Page {number}" aria-current={number === page ? 'page' : undefined} onclick={() => onpage(number)}>{number}</button>
      {/if}
    {/each}
    <button aria-label="Next page" title="Next page" disabled={page >= pages} onclick={() => onpage(page + 1)}><ChevronRight size={17} /></button>
  </nav>
{/if}

<style>
  .pager { display: flex; align-items: center; gap: 3px; }
  button {
    display: grid;
    place-items: center;
    min-width: 34px;
    height: 34px;
    padding: 0 8px;
    background: var(--panel);
    clip-path: var(--stepped);
    color: var(--text-2);
    font-weight: 600;
    transition: background-color 100ms ease-out, color 100ms ease-out;
  }
  button:hover:not(:disabled):not(.active) { background: var(--raised); color: var(--text); }
  button:focus-visible { outline-offset: -3px; }
  button:disabled { color: var(--text-3); opacity: 0.5; cursor: default; }
  button.active { background: var(--accent); color: var(--on-accent); cursor: default; }
  .gap { min-width: 22px; color: var(--text-3); text-align: center; }
</style>

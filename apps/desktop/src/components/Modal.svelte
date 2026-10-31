<script lang="ts">
  import type { Snippet } from 'svelte';
  import { X } from '@lucide/svelte';

  let { title, width = 460, onclose, children }: { title: string; width?: number; onclose: () => void; children: Snippet } = $props();

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') onclose();
  }
</script>

<svelte:window {onkeydown} />

<div class="backdrop">
  <div class="sheet" role="dialog" aria-modal="true" aria-label={title} style:width="{width}px">
    <div class="head">
      <h2>{title}</h2>
      <button class="close" aria-label="Close" onclick={onclose}><X size={15} /></button>
    </div>
    <div class="body">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    padding: 24px;
    background: rgb(5 6 8 / 62%);
  }
  .sheet {
    display: flex;
    flex-direction: column;
    max-width: 100%;
    max-height: 100%;
    border: 1px solid var(--line-strong);
    border-radius: 4px;
    background: var(--panel);
  }
  .head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 40px;
    padding: 0 8px 0 16px;
    border-bottom: 1px solid var(--line);
  }
  h2 { font-size: var(--text-m); }
  .close { display: grid; place-items: center; width: 26px; height: 26px; color: var(--text-2); }
  .close:hover { color: var(--text); }
  .body { overflow-y: auto; padding: 16px; }
</style>

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
    background: rgb(0 0 0 / 55%);
  }
  .sheet {
    display: flex;
    flex-direction: column;
    max-width: 100%;
    max-height: 100%;
    --step: 3px;
    background: var(--panel);
    clip-path: var(--stepped);
  }
  .head {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 56px;
    padding: 0 12px 0 20px;
  }
  h2 { font-size: var(--text-l); }
  .close { display: grid; place-items: center; width: 32px; height: 32px; color: var(--text-2); }
  .close:hover { color: var(--text); }
  .body { overflow-y: auto; padding: 0 20px 20px; }
  .body :global(.form-row) { grid-template-columns: 130px minmax(0, 1fr); }
</style>

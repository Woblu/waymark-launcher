<script lang="ts">
  import { X } from '@lucide/svelte';
  import { app } from '../lib/state.svelte';
</script>

<div class="stack" aria-live="assertive">
  {#each app.toasts as toast (toast.id)}
    <div class="toast" class:bad={toast.tone === 'bad'}>
      <div>
        <strong>{toast.title}</strong>
        {#if toast.detail}<p class="selectable">{toast.detail}</p>{/if}
      </div>
      <button aria-label="Dismiss" onclick={() => app.dismiss(toast.id)}><X size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: 12px;
    bottom: calc(var(--statusbar) + 12px);
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 340px;
  }
  .toast {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--raised);
  }
  strong { font-weight: 600; }
  .bad strong { color: var(--bad); }
  p { margin-top: 2px; color: var(--text-2); font-size: var(--text-xs); line-height: 1.45; }
  button { align-self: flex-start; color: var(--text-3); }
  button:hover { color: var(--text); }
</style>

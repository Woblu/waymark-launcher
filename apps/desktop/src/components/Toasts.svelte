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
      {#if toast.action}
        <button class="action" onclick={() => { toast.action?.run(); app.dismiss(toast.id); }}>{toast.action.label}</button>
      {/if}
      <button aria-label="Dismiss" onclick={() => app.dismiss(toast.id)}><X size={14} /></button>
    </div>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: var(--gutter);
    bottom: calc(var(--playbar) + 14px);
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 340px;
  }
  .toast {
    display: flex;
    gap: 12px;
    padding: 12px 14px;
    background: var(--raised);
    clip-path: var(--stepped);
  }
  .toast > div { flex: 1; min-width: 0; }
  strong { font-weight: 600; }
  .bad strong { color: var(--bad); }
  p { margin-top: 2px; color: var(--text-2); font-size: var(--text-xs); line-height: 1.45; }
  button { align-self: flex-start; color: var(--text-3); }
  button:hover { color: var(--text); }
  .action { color: var(--accent); font-weight: 600; }
  .action:hover { color: var(--accent-hover); }
</style>

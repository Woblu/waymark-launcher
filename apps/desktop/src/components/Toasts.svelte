<script lang="ts">
  import { X } from '@lucide/svelte';
  import { app } from '../lib/state.svelte';
  import type { Toast } from '../lib/state.svelte';

  let copied = $state<number | null>(null);

  async function copyDetails(toast: Toast) {
    if (!toast.details) return;
    try {
      await navigator.clipboard.writeText(toast.details);
      copied = toast.id;
      setTimeout(() => (copied = copied === toast.id ? null : copied), 1600);
    } catch {
      // Nothing to fall back to, and the text is still on screen to select.
    }
  }
</script>

<!-- News is read out when there's a pause, and an error straight away. -->
<div class="stack" aria-live="polite" aria-relevant="additions">
  {#each app.toasts as toast (toast.id)}
    <div class="toast" class:bad={toast.tone === 'bad'} role={toast.tone === 'bad' ? 'alert' : 'status'}>
      <div class="text">
        <strong>{toast.title}</strong>
        <!-- Sentences come a line each, so each one is translated on its own. -->
        {#if toast.detail}<p>{#each toast.detail.split('\n') as line, index (index)}<span>{line}</span>{' '}{/each}</p>{/if}
        {#if toast.technical}<p class="technical selectable">{toast.technical}</p>{/if}
        {#if toast.action || toast.details}
          <div class="actions">
            {#if toast.action}
              <button class="action" onclick={() => { toast.action?.run(); app.dismiss(toast.id); }}>{toast.action.label}</button>
            {/if}
            {#if toast.details}
              <button class="action quiet" onclick={() => copyDetails(toast)}>{copied === toast.id ? 'Copied' : 'Copy details'}</button>
            {/if}
          </div>
        {/if}
      </div>
      <button class="dismiss" aria-label="Dismiss" onclick={() => app.dismiss(toast.id)}><X size={14} /></button>
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
  .text { flex: 1; min-width: 0; }
  strong { font-weight: 600; }
  .bad strong { color: var(--bad); }
  p { margin-top: 2px; color: var(--text-2); font-size: var(--text-xs); line-height: 1.45; overflow-wrap: anywhere; user-select: text; }
  .technical { margin-top: 4px; color: var(--text-3); font-family: var(--mono); }
  .actions { display: flex; flex-wrap: wrap; gap: 4px 14px; margin-top: 6px; }
  button { color: var(--text-3); }
  button:hover { color: var(--text); }
  .dismiss { align-self: flex-start; }
  .action { color: var(--accent-text); font-weight: 600; }
  .action:hover { color: var(--accent-hover); }
  .action.quiet { color: var(--text-2); }
  .action.quiet:hover { color: var(--text); }
</style>

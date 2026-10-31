<script lang="ts">
  import { Minus, Square, X } from '@lucide/svelte';
  import { inTauri } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { View } from '../lib/state.svelte';
  import Logo from './Logo.svelte';

  const VIEWS: { id: View; label: string }[] = [
    { id: 'library', label: 'Library' },
    { id: 'discover', label: 'Discover' },
    { id: 'settings', label: 'Settings' },
  ];

  async function windowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
    if (!inTauri) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow()[action]();
  }

  const account = $derived(app.activeAccount);
</script>

<header data-tauri-drag-region>
  <span class="mark"><Logo /></span>
  <nav aria-label="Main">
    {#each VIEWS as view (view.id)}
      <button class:active={app.view === view.id} aria-current={app.view === view.id ? 'page' : undefined} onclick={() => app.go(view.id)}>{view.label}</button>
    {/each}
  </nav>
  <div class="right">
    <button class="account" onclick={() => (account ? app.go('settings') : (app.dialog = 'signin'))}>
      {account ? account.name : 'Sign in'}
    </button>
    <div class="controls">
      <button aria-label="Minimize" onclick={() => windowAction('minimize')}><Minus size={14} /></button>
      <button aria-label="Maximize" onclick={() => windowAction('toggleMaximize')}><Square size={11} /></button>
      <button class="close" aria-label="Close" onclick={() => windowAction('close')}><X size={15} /></button>
    </div>
  </div>
</header>

<style>
  header {
    flex: none;
    display: flex;
    align-items: center;
    height: var(--titlebar);
    padding-left: 14px;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
  }
  .mark { display: flex; margin-right: 16px; }
  nav { display: flex; height: 100%; gap: 2px; }
  nav button { padding: 0 11px; color: var(--text-2); }
  nav button:hover { color: var(--text); }
  nav button.active { color: var(--text); font-weight: 600; }
  .right { display: flex; align-items: center; height: 100%; margin-left: auto; }
  .account { height: 100%; padding: 0 14px; color: var(--text-2); }
  .account:hover { color: var(--text); }
  .controls { display: flex; height: 100%; }
  .controls button { display: grid; place-items: center; width: 44px; height: 100%; color: var(--text-2); }
  .controls button:hover { background: var(--control-hover); color: var(--text); }
  .controls .close:hover { background: #c42b1c; color: #fff; }
</style>

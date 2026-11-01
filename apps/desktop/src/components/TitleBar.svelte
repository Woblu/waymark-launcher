<script lang="ts">
  import { ArrowLeft, Minus, Settings, Square, UserRound, X } from '@lucide/svelte';
  import { api, inTauri } from '../lib/api';
  import { skinHead } from '../lib/skin-render';
  import { app } from '../lib/state.svelte';
  import type { View } from '../lib/state.svelte';
  import Logo from './Logo.svelte';

  const VIEWS: { id: View; label: string }[] = [
    { id: 'library', label: 'Library' },
    { id: 'discover', label: 'Discover' },
    { id: 'skins', label: 'Skins' },
    { id: 'screenshots', label: 'Screenshots' },
    { id: 'friends', label: 'Friends' },
  ];

  const account = $derived(app.activeAccount);

  let head = $state<string | null>(null);

  $effect(() => {
    const name = account?.name;
    head = null;
    if (!name) return;
    api.skinProfile().then(
      async (profile) => {
        const worn = profile?.skins.find((skin) => skin.state === 'ACTIVE');
        if (worn && account?.name === name) head = await skinHead(worn.url);
      },
      () => {},
    );
  });

  async function windowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
    if (!inTauri) return;
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    await getCurrentWindow()[action]();
  }
</script>

<header data-tauri-drag-region>
  <span class="brand" data-tauri-drag-region><Logo size={24} /><span class="word" data-tauri-drag-region>Waymark</span></span>
  <button class="back" class:shown={app.canGoBack} aria-label="Back" title="Back" tabindex={app.canGoBack ? 0 : -1} onclick={() => app.back()}><ArrowLeft size={18} /></button>
  <nav class="main-nav" aria-label="Main">
    {#each VIEWS as view (view.id)}
      <button class:active={app.view === view.id} aria-current={app.view === view.id ? 'page' : undefined} onclick={() => app.go(view.id)}>
        {view.label}{#if view.id === 'friends' && app.friendAlerts}<span class="count tabular">{app.friendAlerts}</span>{/if}
      </button>
    {/each}
  </nav>
  <div class="right" data-tauri-drag-region>
    <button class="you" onclick={() => (account ? app.go('settings') : (app.dialog = 'signin'))}>
      {#if head}<img src={head} alt="" />{:else}<UserRound size={17} />{/if}
      <span class="truncate name">{account ? account.name : 'Sign in'}</span>
    </button>
    <button class="settings" class:active={app.view === 'settings'} aria-label="Settings" aria-current={app.view === 'settings' ? 'page' : undefined} title="Settings" onclick={() => app.go('settings')}><Settings size={18} /></button>
    <div class="controls">
      <button aria-label="Minimize" onclick={() => windowAction('minimize')}><Minus size={15} /></button>
      <button aria-label="Maximize" onclick={() => windowAction('toggleMaximize')}><Square size={12} /></button>
      <button class="close" aria-label="Close" onclick={() => windowAction('close')}><X size={16} /></button>
    </div>
  </div>
</header>

<style>
  header {
    flex: none;
    display: flex;
    align-items: stretch;
    height: var(--titlebar);
    padding-left: var(--gutter);
  }
  .brand { flex: none; display: flex; align-items: center; gap: 10px; }
  .word { font-size: var(--text-l); font-weight: 800; letter-spacing: -0.02em; }
  .back { flex: none; display: grid; place-items: center; width: 32px; margin: 0 2px 0 14px; color: var(--text-2); visibility: hidden; }
  .back.shown { visibility: visible; }
  .back:hover { color: var(--text); }

  .main-nav { display: flex; gap: 2px; }
  .main-nav button { position: relative; padding: 0 12px; color: var(--text-2); font-size: var(--text-m); font-weight: 600; transition: color 100ms ease-out; }
  .main-nav button:hover { color: var(--text); }
  .main-nav button.active { color: var(--text); }
  /* A small lit marker under the page you're on. */
  .main-nav button.active::after { content: ''; position: absolute; bottom: 7px; left: 50%; width: 6px; height: 6px; margin-left: -3px; background: var(--accent); }

  .count { display: inline-block; min-width: 16px; margin-left: 5px; padding: 0 4px; background: var(--accent); color: var(--on-accent); font-size: var(--text-xs); font-weight: 700; line-height: 16px; text-align: center; }
  .right { flex: 1; display: flex; align-items: center; justify-content: flex-end; gap: 6px; min-width: 0; }
  .you { display: flex; align-items: center; gap: 8px; min-width: 0; max-width: 200px; height: 34px; padding: 0 10px; color: var(--text-2); font-weight: 600; }
  .you:hover { color: var(--text); }
  .you img { display: block; width: 22px; height: 22px; image-rendering: pixelated; }
  .settings { display: grid; place-items: center; width: 34px; height: 34px; color: var(--text-2); }
  .settings:hover { color: var(--text); }
  .settings.active { color: var(--accent); }
  .controls { flex: none; display: flex; align-self: stretch; margin-left: 6px; }
  .controls button { display: grid; place-items: center; width: 46px; color: var(--text-3); }
  .controls button:hover { background: var(--panel); color: var(--text); }
  .controls .close:hover { background: #c42b1c; color: #fff; }
</style>

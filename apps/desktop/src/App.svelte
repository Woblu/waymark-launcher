<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from './lib/api';
  import { app, showWindow } from './lib/state.svelte';
  import { useTheme } from './lib/theme';
  import PlayBar from './components/PlayBar.svelte';
  import TitleBar from './components/TitleBar.svelte';
  import Toasts from './components/Toasts.svelte';
  import NewInstance from './components/NewInstance.svelte';
  import SignIn from './components/SignIn.svelte';
  import ContextMenu from './components/ContextMenu.svelte';
  import Dropdowns from './components/Dropdowns.svelte';
  import CrashSheet from './components/CrashSheet.svelte';
  import InstanceDialogs from './components/InstanceDialogs.svelte';
  import Library from './pages/Library.svelte';
  import Discover from './pages/Discover.svelte';
  import Friends from './pages/Friends.svelte';
  import Project from './pages/Project.svelte';
  import Screenshots from './pages/Screenshots.svelte';
  import Settings from './pages/Settings.svelte';
  import Skins from './pages/Skins.svelte';

  onMount(async () => {
    await app.start();
    api.settings().then((settings) => useTheme(settings.theme, settings.customThemes), () => {});
    await showWindow();
  });
</script>

<div class="shell">
  <TitleBar />
  <main>
    <!-- Stays mounted under a project page, so Back keeps the search and scroll position. -->
    <div class="view" hidden={!!app.project}>
      {#if app.view === 'library'}
        <Library />
      {:else if app.view === 'discover'}
        <Discover target={app.discoverTarget} />
      {:else if app.view === 'skins'}
        <Skins />
      {:else if app.view === 'screenshots'}
        <Screenshots />
      {:else if app.view === 'friends'}
        <Friends />
      {:else}
        <Settings />
      {/if}
    </div>
    {#if app.project}
      {#key app.project}<Project target={app.project} />{/key}
    {/if}
  </main>
  <PlayBar />
  <Toasts />
  {#if app.dialog === 'new'}<NewInstance />{/if}
  {#if app.dialog === 'signin'}<SignIn />{/if}
  {#if app.instanceDialog}{#key app.instanceDialog}<InstanceDialogs dialog={app.instanceDialog} />{/key}{/if}
  {#if app.crash}<CrashSheet />{/if}
  <ContextMenu />
  <Dropdowns />
  {#if app.dragging}
    <div class="drop"><p>Drop to install</p></div>
  {/if}
</div>

<style>
  .shell { display: flex; flex-direction: column; height: 100%; }
  main { flex: 1; min-height: 0; overflow: hidden; }
  .view { height: 100%; }
  .drop { position: fixed; inset: 0; z-index: 70; display: grid; place-items: center; background: rgb(0 0 0 / 55%); pointer-events: none; }
  .drop p { padding: 16px 26px; border: 2px dashed var(--text-2); background: var(--raised); font-size: var(--text-l); font-weight: 600; }
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import { app, showWindow } from './lib/state.svelte';
  import TitleBar from './components/TitleBar.svelte';
  import StatusBar from './components/StatusBar.svelte';
  import Toasts from './components/Toasts.svelte';
  import NewInstance from './components/NewInstance.svelte';
  import SignIn from './components/SignIn.svelte';
  import CrashSheet from './components/CrashSheet.svelte';
  import Library from './pages/Library.svelte';
  import Discover from './pages/Discover.svelte';
  import Settings from './pages/Settings.svelte';

  onMount(async () => {
    await app.start();
    await showWindow();
  });
</script>

<div class="shell">
  <TitleBar />
  <main>
    {#if app.view === 'library'}
      <Library />
    {:else if app.view === 'discover'}
      <Discover target={app.discoverTarget} />
    {:else}
      <Settings />
    {/if}
  </main>
  <StatusBar />
  <Toasts />
  {#if app.dialog === 'new'}<NewInstance />{/if}
  {#if app.dialog === 'signin'}<SignIn />{/if}
  {#if app.crash}<CrashSheet />{/if}
</div>

<style>
  .shell { display: flex; flex-direction: column; height: 100%; }
  main { flex: 1; min-height: 0; }
</style>

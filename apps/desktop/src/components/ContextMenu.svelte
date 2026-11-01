<script lang="ts">
  import { onMount } from 'svelte';
  import { app } from '../lib/state.svelte';

  const menu = $derived(app.menu);
  let element = $state<HTMLDivElement>();
  let left = $state(0);
  let top = $state(0);
  const MARGIN = 6;

  // Opens at the pointer, moved in if it would run off the window.
  $effect(() => {
    if (!menu || !element) return;
    left = Math.min(menu.x, innerWidth - element.offsetWidth - MARGIN);
    top = menu.up ? menu.y - element.offsetHeight : Math.min(menu.y, innerHeight - element.offsetHeight - MARGIN);
    element.querySelector('button')?.focus();
  });

  const close = () => (app.menu = null);

  function move(event: KeyboardEvent) {
    const buttons = [...(element?.querySelectorAll('button') ?? [])];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === 'ArrowDown') buttons[(at + 1) % buttons.length]?.focus();
    else if (event.key === 'ArrowUp') buttons[(at - 1 + buttons.length) % buttons.length]?.focus();
    else if (event.key === 'Escape') close();
    else return;
    event.preventDefault();
  }

  onMount(() => {
    const away = () => close();
    addEventListener('blur', away);
    addEventListener('resize', away);
    return () => {
      removeEventListener('blur', away);
      removeEventListener('resize', away);
    };
  });
</script>

<svelte:window onpointerdown={(event) => { if (menu && !element?.contains(event.target as Node)) close(); }} />

{#if menu}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="menu" role="menu" tabindex="-1" bind:this={element} style:left="{left}px" style:top="{top}px" onkeydown={move}>
    {#each menu.items as item (item.label)}
      <button role="menuitem" class:danger={item.danger} onclick={() => { close(); item.run(); }}>{item.label}</button>
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 60;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    max-height: 60vh;
    overflow-y: auto;
    padding: 4px;
    background: var(--raised);
    clip-path: var(--stepped);
  }
  button { padding: 8px 10px; font-weight: 500; text-align: left; white-space: nowrap; }
  button:hover, button:focus-visible { background: var(--control); outline: none; }
  .danger { color: var(--bad); }
</style>

<script lang="ts">
  import { untrack } from 'svelte';
  import { ChevronLeft, ChevronRight, ExternalLink, X } from '@lucide/svelte';
  import { openExternal } from '../lib/api';
  import type { Shot } from '../lib/api';

  let { shots, start, onclose }: { shots: Shot[]; start: number; onclose: () => void } = $props();

  let index = $state(untrack(() => start));
  let close = $state<HTMLButtonElement>();

  const shot = $derived(shots[index]);
  const several = $derived(shots.length > 1);

  $effect(() => close?.focus());

  const step = (by: number) => (index = (index + by + shots.length) % shots.length);

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') onclose();
    else if (event.key === 'ArrowLeft' && several) step(-1);
    else if (event.key === 'ArrowRight' && several) step(1);
    else return;
    event.stopPropagation();
    event.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<!-- The buttons cover the keyboard. Clicking the backdrop is a shortcut for the mouse. -->
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<div class="lightbox" role="dialog" aria-modal="true" aria-label="Image viewer" tabindex="-1" onclick={(event) => event.target === event.currentTarget && onclose()}>
  <div class="bar">
    {#if several}<span class="muted tabular">{index + 1} of {shots.length}</span>{/if}
    <span class="spacer"></span>
    {#if shot.full.startsWith('https://')}
      <button class="btn" onclick={() => openExternal(shot.full)}>Open original <ExternalLink size={13} /></button>
    {/if}
    <button class="btn icon" aria-label="Close" bind:this={close} onclick={onclose}><X size={16} /></button>
  </div>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="stage" onclick={(event) => event.target === event.currentTarget && onclose()}>
    {#if several}<button class="turn" aria-label="Previous image" onclick={() => step(-1)}><ChevronLeft size={26} /></button>{/if}
    {#key shot.full}<img src={shot.full} alt={shot.title ?? ''} referrerpolicy="no-referrer" />{/key}
    {#if several}<button class="turn next" aria-label="Next image" onclick={() => step(1)}><ChevronRight size={26} /></button>{/if}
  </div>
  {#if shot.title || shot.description}
    <div class="caption">
      {#if shot.title}<p class="title">{shot.title}</p>{/if}
      {#if shot.description}<p class="muted">{shot.description}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .lightbox {
    position: fixed;
    inset: var(--titlebar) 0 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    background: rgb(7 8 10 / 95%);
  }
  .bar { flex: none; display: flex; align-items: center; gap: 8px; padding: 10px 14px; }
  .spacer { flex: 1; }
  .stage { position: relative; flex: 1; min-height: 0; display: flex; align-items: center; justify-content: center; padding: 0 60px; }
  img { max-width: 100%; max-height: 100%; object-fit: contain; }
  .turn {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    display: grid;
    place-items: center;
    width: 60px;
    color: var(--text-2);
  }
  .turn.next { right: 0; left: auto; }
  .turn:hover { color: var(--text); background: rgb(255 255 255 / 4%); }
  .caption { flex: none; max-width: 760px; margin: 0 auto; padding: 12px 24px 16px; text-align: center; }
  .title { font-size: var(--text-m); font-weight: 600; }
</style>

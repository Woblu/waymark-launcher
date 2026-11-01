<script lang="ts">
  import { Play, Square } from '@lucide/svelte';
  import type { LibraryEntry } from '../lib/api';
  import { app } from '../lib/state.svelte';

  let { entry }: { entry: LibraryEntry } = $props();

  const starting = $derived(app.tasks.some((t) => t.title === `Launching ${entry.name}`));
</script>

{#if entry.running}
  <button class="btn play" onclick={() => app.stop(entry.id)}><Square size={13} fill="currentColor" /> Stop</button>
{:else if starting}
  <button class="btn play" disabled>Starting</button>
{:else}
  <button class="btn primary play" onclick={() => app.play(entry.id)}><Play size={16} fill="currentColor" /> Play</button>
{/if}

<style>
  .play { --step: 3px; min-width: 150px; height: 46px; font-size: var(--text-l); font-weight: 700; }
</style>

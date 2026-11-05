<script lang="ts">
  import { Play, Square } from '@lucide/svelte';
  import type { LibraryEntry } from '../lib/api';
  import { app } from '../lib/state.svelte';

  let { entry }: { entry: LibraryEntry } = $props();

  // Task events carry only an id and a title, and the core names a launch
  // `Launching <instance name>`, so the title is all there is to match. It comes
  // from Rust in English whatever the interface language, so translation can't
  // break it, but renaming the instance mid-launch or rewording the core's title would.
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

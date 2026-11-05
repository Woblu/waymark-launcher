<script lang="ts">
  // One quiet line for pages that need the internet, shown for as long as it's away.
  import { WifiOff } from '@lucide/svelte';
  import { app } from '../lib/state.svelte';

  let { what, onretry }: { what: string; onretry?: () => void } = $props();
</script>

<p class="offline" role="status">
  <WifiOff size={15} />
  <!-- The computer saying it's offline is certain. A failed request only says nothing got through. -->
  <span>{app.online ? `Can't reach the internet right now. ${what}` : `You're offline. ${what}`}</span>
  {#if onretry}<button class="retry" onclick={onretry}>Try again</button>{/if}
</p>

<style>
  .offline { display: flex; align-items: center; gap: 8px; margin-bottom: 12px; color: var(--text-2); }
  .offline :global(svg) { flex: none; color: var(--text-3); }
  span { flex: 1; min-width: 0; }
  .retry { color: var(--text-2); font-weight: 600; text-decoration: underline; text-underline-offset: 2px; }
  .retry:hover { color: var(--text); }
</style>

<script lang="ts">
  import VirtualList from '../components/VirtualList.svelte';
  import { app } from '../lib/state.svelte';
  import type { LogLine } from '../lib/state.svelte';

  let { id }: { id: string } = $props();

  let query = $state('');
  let errorsOnly = $state(false);

  const lines = $derived.by(() => {
    app.logTick;
    const all = app.logs.get(id) ?? [];
    const text = query.trim().toLowerCase();
    if (!text && !errorsOnly) return all.slice();
    return all.filter((line) => (!errorsOnly || line.error) && (!text || line.text.toLowerCase().includes(text)));
  });

  function tone(line: LogLine): string {
    if (line.error) return 'error';
    return line.text.includes('/WARN]') ? 'warn' : '';
  }

  async function copyAll() {
    await navigator.clipboard.writeText(lines.map((l) => l.text).join('\n'));
    app.toast('good', `Copied ${lines.length} lines`);
  }
</script>

<div class="tab">
  <div class="tools">
    <input class="field search" placeholder="Search the log" aria-label="Search the log" bind:value={query} />
    <label class="check"><input type="checkbox" bind:checked={errorsOnly} /> Errors only</label>
    <span class="spacer"></span>
    <button class="btn" disabled={!lines.length} onclick={copyAll}>Copy</button>
  </div>

  <div class="console">
    {#if lines.length}
      <VirtualList items={lines} rowHeight={20} stickToEnd>
        {#snippet row(line: LogLine)}
          <div class="line mono selectable {tone(line)}">{line.text}</div>
        {/snippet}
      </VirtualList>
    {:else}
      <p class="faint">Output from the game appears here while it runs.</p>
    {/if}
  </div>
</div>

<style>
  .tab { display: flex; flex-direction: column; height: 100%; }
  .tools { flex: none; display: flex; align-items: center; gap: 12px; padding: 10px 24px; }
  .search { width: 240px; }
  .check { display: flex; align-items: center; gap: 6px; color: var(--text-2); }
  .spacer { flex: 1; }
  .console { flex: 1; min-height: 0; padding: 8px 8px 8px 24px; background: #08090b; border-top: 1px solid var(--line); }
  .line { overflow: hidden; color: var(--text-2); font-size: 12px; line-height: 20px; text-overflow: ellipsis; white-space: pre; }
  .warn { color: var(--warn); }
  .error { color: var(--bad); }
</style>

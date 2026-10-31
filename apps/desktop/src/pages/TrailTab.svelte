<script lang="ts">
  import { api } from '../lib/api';
  import type { Pending, Waymark } from '../lib/api';
  import { bytes, timeAgo } from '../lib/format';
  import { app } from '../lib/state.svelte';

  let { id }: { id: string } = $props();

  let trail = $state<Waymark[]>([]);
  let pending = $state<Pending>({ added: [], removed: [], changed: [] });
  let selected = $state<string | null>(null);
  let label = $state('');
  let working = $state(false);

  const entries = $derived([...trail].reverse());
  const chosen = $derived(entries.find((w) => w.id === selected) ?? entries[0]);
  const isLatest = $derived(chosen?.id === entries[0]?.id);
  const changes = $derived(pending.added.length + pending.removed.length + pending.changed.length);
  const running = $derived(app.entry(id)?.running ?? false);
  const groups = $derived(
    chosen
      ? [
          { title: 'Added', items: chosen.added },
          { title: 'Removed', items: chosen.removed },
          { title: 'Changed', items: chosen.changed },
        ].filter((g) => g.items.length)
      : [],
  );

  async function load() {
    try {
      [trail, pending] = await Promise.all([api.trail(id), api.pending(id)]);
    } catch (error) {
      app.fail(error, 'Could not read the waymarks');
    }
  }

  $effect(() => {
    app.contentTick;
    load();
  });

  async function setWaymark(event: SubmitEvent) {
    event.preventDefault();
    working = true;
    try {
      await api.setWaymark(id, label.trim() || 'Waymark');
      label = '';
      selected = null;
      await load();
      await app.refresh();
    } catch (error) {
      app.fail(error);
    }
    working = false;
  }

  async function goBack(mark: Waymark) {
    working = true;
    try {
      await api.returnTo(id, mark.id);
      selected = null;
      app.contentTick++;
      await app.refresh();
      app.toast('good', `Returned to "${mark.label}"`, 'Your previous state is still in the list.');
    } catch (error) {
      app.fail(error, 'Could not return to that waymark');
    }
    working = false;
  }

  function shortName(path: string): string {
    return path.split('/').pop() ?? path;
  }
</script>

<div class="tab">
  <div class="stream">
    <form class="tools" onsubmit={setWaymark}>
      <span class="muted truncate state">{changes ? `${changes} ${changes === 1 ? 'change' : 'changes'} since the last waymark` : 'No changes since the last waymark'}</span>
      <input class="field" placeholder="Name" aria-label="Waymark name" bind:value={label} />
      <button class="btn" disabled={working || !changes}>Set waymark</button>
    </form>

    <div class="columns">
      <span>Waymark</span>
      <span>Files</span>
      <span class="right">When</span>
    </div>
    <ol>
      {#each entries as mark (mark.id)}
        <li>
          <button class="row" class:chosen={chosen?.id === mark.id} aria-pressed={chosen?.id === mark.id} onclick={() => (selected = mark.id)}>
            <span class="truncate">{mark.label}</span>
            <span class="delta mono">
              {#if mark.added.length}<i class="plus">+{mark.added.length}</i>{/if}
              {#if mark.removed.length}<i class="minus">-{mark.removed.length}</i>{/if}
              {#if mark.changed.length}<i class="tilde">~{mark.changed.length}</i>{/if}
            </span>
            <span class="faint right">{timeAgo(mark.at)}</span>
          </button>
        </li>
      {/each}
    </ol>
  </div>

  {#if chosen}
    <aside aria-label="Selected waymark">
      <h2>{chosen.label}</h2>
      <p class="faint">{new Date(chosen.at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })}</p>

      <dl>
        <dt>Mods</dt><dd>{chosen.mods}</dd>
        <dt>Files</dt><dd>{chosen.files}</dd>
        <dt>Size</dt><dd>{bytes(chosen.bytes)}</dd>
      </dl>

      <div class="changes">
        {#each groups as group (group.title)}
          <h3>{group.title} ({group.items.length})</h3>
          <ul>
            {#each group.items.slice(0, 12) as path (path)}<li class="mono truncate">{shortName(path)}</li>{/each}
            {#if group.items.length > 12}<li class="faint">and {group.items.length - 12} more</li>{/if}
          </ul>
        {:else}
          <p class="faint">No files changed at this point.</p>
        {/each}
      </div>

      {#if isLatest}
        <p class="faint">This is the newest waymark. Select an earlier one to go back.</p>
      {:else}
        <button class="btn primary back" disabled={working || running} onclick={() => goBack(chosen)}>Return to this waymark</button>
        <p class="faint small">{running ? 'Close the game first.' : 'Your worlds are left alone, and this can be undone.'}</p>
      {/if}
    </aside>
  {/if}
</div>

<style>
  .tab { display: grid; grid-template-columns: minmax(0, 1fr) 300px; height: 100%; }
  .stream { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .tools { flex: none; display: flex; align-items: center; gap: 8px; padding: 10px 16px 10px 24px; }
  .state { flex: 1; }
  .tools .field { width: 170px; }
  .columns, .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 90px 100px;
    gap: 10px;
    align-items: center;
    padding: 0 16px 0 24px;
  }
  .columns {
    flex: none;
    height: 26px;
    border-top: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
    color: var(--text-3);
    font-size: var(--text-xs);
  }
  .right { text-align: right; }
  ol { flex: 1; min-height: 0; overflow-y: auto; }
  .row { width: 100%; height: 34px; border-bottom: 1px solid var(--line); text-align: left; }
  .row:hover { background: var(--panel); }
  .row.chosen { background: var(--selected); }
  .delta { display: flex; gap: 8px; font-size: var(--text-xs); }
  .delta i { font-style: normal; }
  .plus { color: var(--good); }
  .minus { color: var(--bad); }
  .tilde { color: var(--info); }

  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 14px 18px;
    background: var(--panel);
    border-left: 1px solid var(--line);
  }
  h2 { font-size: var(--text-l); line-height: 1.3; }
  dl { display: grid; grid-template-columns: 60px 1fr; gap: 3px 10px; margin: 12px 0; padding: 10px 0; border-block: 1px solid var(--line); }
  dt { color: var(--text-3); }
  .changes { flex: 1; min-height: 0; overflow-y: auto; }
  h3 { margin: 0 0 3px; color: var(--text-2); font-size: var(--text-s); font-weight: 600; }
  ul { margin-bottom: 12px; }
  ul li { font-size: var(--text-xs); line-height: 1.7; }
  .back { width: 100%; margin: 10px 0 6px; }
</style>

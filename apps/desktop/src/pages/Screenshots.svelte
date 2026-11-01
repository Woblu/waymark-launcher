<script lang="ts">
  import { FolderOpen, Trash2 } from '@lucide/svelte';
  import Lightbox from '../components/Lightbox.svelte';
  import { api, localImage, openFolder } from '../lib/api';
  import type { Screenshot, Shot } from '../lib/api';
  import { timeAgo } from '../lib/format';
  import { app } from '../lib/state.svelte';

  const SHOWN = 60;

  let all = $state<Screenshot[]>([]);
  let loaded = $state(false);
  let instance = $state('');
  let limit = $state(SHOWN);
  let viewing = $state<number | null>(null);

  const source = (shot: Screenshot) => localImage(`screenshot/${shot.instance}/${shot.file}`) ?? '';
  const filtered = $derived(instance ? all.filter((shot) => shot.instance === instance) : all);
  const shown = $derived(filtered.slice(0, limit));
  const instances = $derived([...new Map(all.map((shot) => [shot.instance, shot.instanceName])).entries()]);
  const shots = $derived(filtered.map((shot): Shot => ({ url: source(shot), full: source(shot), title: shot.file })));

  $effect(() => {
    app.contentTick;
    api.screenshots().then(
      (found) => {
        all = found;
        loaded = true;
      },
      (error) => app.fail(error, 'Could not read your screenshots'),
    );
  });

  async function remove(shot: Screenshot) {
    try {
      const item = await api.deleteScreenshot(shot.instance, shot.file);
      app.contentTick++;
      app.toast('good', 'Screenshot deleted', 'You can restore it from Settings for 30 days.', { label: 'Undo', run: () => app.restore(item.id) });
    } catch (error) {
      app.fail(error, 'Could not delete that screenshot');
    }
  }

  async function openShots() {
    const id = instance || all[0]?.instance;
    if (id) await openFolder(`${await api.gameDirectory(id)}/screenshots`);
  }
</script>

<div class="page">
  <div class="head">
    <h1>Screenshots</h1>
    <span class="faint tabular">{filtered.length}</span>
    <span class="spacer"></span>
    {#if instances.length > 1}
      <select class="field which" aria-label="Instance" bind:value={instance}>
        <option value="">All instances</option>
        {#each instances as [id, name] (id)}<option value={id}>{name}</option>{/each}
      </select>
    {/if}
    <button class="btn" disabled={!all.length} onclick={openShots}><FolderOpen size={16} /> Open folder</button>
  </div>

  {#if loaded && !all.length}
    <div class="blank">
      <h2>No screenshots yet</h2>
      <p class="muted">Press F2 in the game to take a screenshot, and it shows up here.</p>
    </div>
  {/if}

  <ul>
    {#each shown as shot, index (shot.instance + shot.file)}
      <li>
        <button class="picture" aria-label="View {shot.file}" onclick={() => (viewing = index)}>
          <img src={source(shot)} alt="" loading="lazy" />
        </button>
        <button class="remove" aria-label="Delete {shot.file}" title="Delete" onclick={() => remove(shot)}><Trash2 size={14} /></button>
        <p class="truncate small">{shot.instanceName}<span class="faint"> · {timeAgo(new Date(shot.modified * 1000).toISOString())}</span></p>
      </li>
    {/each}
  </ul>

  {#if filtered.length > limit}
    <div class="more"><button class="btn" onclick={() => (limit += SHOWN)}>Show more</button></div>
  {/if}
</div>

{#if viewing !== null}<Lightbox {shots} start={viewing} onclose={() => (viewing = null)} />{/if}

<style>
  .page { height: 100%; overflow-y: auto; padding: 20px var(--gutter) var(--gutter); }
  .head { display: flex; align-items: center; gap: 10px; margin-bottom: 16px; }
  .spacer { flex: 1; }
  .which { width: 200px; }
  ul { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 14px; }
  li { position: relative; min-width: 0; content-visibility: auto; contain-intrinsic-size: auto 190px; }
  .picture { display: block; width: 100%; aspect-ratio: 16 / 9; overflow: hidden; background: var(--panel); clip-path: var(--stepped); cursor: zoom-in; }
  .picture img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .remove { position: absolute; top: 8px; right: 8px; display: grid; place-items: center; width: 30px; height: 30px; background: var(--control); clip-path: var(--stepped); color: var(--text-2); opacity: 0; }
  li:hover .remove, .remove:focus-visible { opacity: 1; }
  .remove:hover { color: var(--bad); }
  p { margin-top: 6px; }
  .more { padding-top: 14px; }
</style>

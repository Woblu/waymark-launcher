<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { api } from '../lib/api';
  import type { ContentHit, ContentKind, ContentSource } from '../lib/api';
  import { count, loaderName } from '../lib/format';
  import { app } from '../lib/state.svelte';

  let { target }: { target?: string } = $props();

  const KINDS: { id: ContentKind; label: string }[] = [
    { id: 'mod', label: 'Mods' },
    { id: 'modpack', label: 'Modpacks' },
    { id: 'shader', label: 'Shaders' },
    { id: 'resourcepack', label: 'Resource packs' },
  ];
  const PAGE = 30;
  const SEARCH_DELAY = 240;

  let source = $state<ContentSource>('modrinth');
  let kind = $state<ContentKind>(untrack(() => (target ? 'mod' : 'modpack')));
  let text = $state('');
  let instanceId = $state(untrack(() => target ?? app.current?.id ?? ''));
  let hits = $state<ContentHit[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let hasCurseForge = $state(false);
  let installing = $state<string | null>(null);
  let installed = $state(new Set<string>());

  const instance = $derived(app.entry(instanceId));
  let requestId = 0;

  async function run(offset: number) {
    const request = ++requestId;
    loading = true;
    try {
      const filters = instance && kind !== 'modpack' ? { minecraft: instance.minecraft, loader: instance.loader.kind } : {};
      const page = await api.search(source, { text: text.trim(), kind, offset, limit: PAGE, ...filters });
      if (request !== requestId) return;
      hits = offset ? [...hits, ...page.hits] : page.hits;
      total = page.total;
    } catch (error) {
      if (request === requestId) app.fail(error, 'Search is not available right now');
    }
    if (request === requestId) loading = false;
  }

  onMount(async () => {
    hasCurseForge = await api.curseforgeAvailable();
  });

  $effect(() => {
    source;
    kind;
    text;
    instanceId;
    const timer = setTimeout(() => untrack(() => run(0)), SEARCH_DELAY);
    return () => clearTimeout(timer);
  });

  async function install(hit: ContentHit) {
    installing = hit.id;
    try {
      if (hit.kind === 'modpack') {
        const result = await api.installPack(hit.source, hit.id);
        await app.refresh();
        for (const file of result.manual) app.toast('info', `${file.name} needs a manual download`, file.page);
        app.open(result.instance.id);
      } else {
        await api.installContent(instanceId, hit.source, hit.id, hit.kind);
        installed = new Set(installed).add(hit.id);
        await app.refresh();
        app.contentTick++;
      }
    } catch (error) {
      app.fail(error, `Could not install ${hit.title}`);
    }
    installing = null;
  }
</script>

<div class="page">
  <div class="toolbar">
    <nav class="tabs" aria-label="Content type">
      {#each KINDS as item (item.id)}
        <button class:active={kind === item.id} onclick={() => (kind = item.id)}>{item.label}</button>
      {/each}
    </nav>
    <div class="filters">
      <input class="field search" placeholder="Search" aria-label="Search" bind:value={text} />
      <label for="source" class="muted">Source</label>
      <select id="source" class="field source" bind:value={source}>
        <option value="modrinth">Modrinth</option>
        <option value="curseforge" disabled={!hasCurseForge}>CurseForge</option>
      </select>
      {#if kind !== 'modpack' && app.library.length}
        <label for="target" class="muted">Add to</label>
        <select id="target" class="field target" bind:value={instanceId}>
          {#each app.library as entry (entry.id)}<option value={entry.id}>{entry.name}</option>{/each}
        </select>
        {#if instance}<span class="faint small truncate">{instance.minecraft} {loaderName(instance.loader.kind)}</span>{/if}
      {/if}
    </div>
  </div>

  <div class="results">
    <ul>
      {#each hits as hit (`${hit.source}:${hit.id}`)}
        <li>
          <span class="icon">{#if hit.icon}<img src={hit.icon} alt="" loading="lazy" />{:else}{hit.title.charAt(0).toUpperCase()}{/if}</span>
          <div class="info">
            <p class="truncate"><span class="title">{hit.title}</span> <span class="faint">by {hit.author}</span></p>
            <p class="muted summary">{hit.summary}</p>
          </div>
          <span class="faint tabular downloads">{count(hit.downloads)} downloads</span>
          {#if installed.has(hit.id)}
            <span class="added">Added</span>
          {:else}
            <button class="btn" disabled={installing !== null || (hit.kind !== 'modpack' && !instanceId)} onclick={() => install(hit)}>
              {installing === hit.id ? 'Installing' : hit.kind === 'modpack' ? 'Install' : 'Add'}
            </button>
          {/if}
        </li>
      {/each}
    </ul>

    {#if !loading && !hits.length}
      <p class="none muted">Nothing found.</p>
    {/if}
    {#if hits.length && hits.length < total}
      <div class="more"><button class="btn" disabled={loading} onclick={() => run(hits.length)}>{loading ? 'Loading' : 'Show more'}</button></div>
    {/if}
  </div>
</div>

<style>
  .page { display: flex; flex-direction: column; height: 100%; }
  .toolbar { flex: none; padding: 6px 24px 0; }
  .filters { display: flex; align-items: center; gap: 8px; padding: 10px 0; }
  .search { width: 260px; }
  .source { width: 120px; }
  .target { width: 200px; }
  .filters label { margin-left: 8px; }
  .results { flex: 1; min-height: 0; overflow-y: auto; }
  li {
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr) 118px 74px;
    gap: 12px;
    align-items: center;
    padding: 9px 24px;
    border-top: 1px solid var(--line);
    content-visibility: auto;
    contain-intrinsic-size: auto 62px;
  }
  li:hover { background: var(--panel); }
  .icon { display: grid; place-items: center; width: 40px; height: 40px; overflow: hidden; background: var(--control); color: var(--text-3); font-size: 18px; font-weight: 600; }
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .info { min-width: 0; }
  .title { font-size: var(--text-m); font-weight: 600; }
  .summary {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }
  .downloads { text-align: right; }
  li .btn { width: 100%; }
  .added { color: var(--good); text-align: center; }
  .none { padding: 20px 24px; }
  .more { padding: 14px 24px; border-top: 1px solid var(--line); }
</style>

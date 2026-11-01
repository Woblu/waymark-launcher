<script lang="ts" module>
  import type { ContentKind, ContentSource, SearchCategory } from '../lib/api';

  /** The lists barely change, so one load a session is enough. */
  const categoryLists = new Map<string, SearchCategory[]>();
</script>

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { Check, Download, Search } from '@lucide/svelte';
  import Art from '../components/Art.svelte';
  import Pager from '../components/Pager.svelte';
  import { api } from '../lib/api';
  import type { ContentHit, LoaderKind, Sort } from '../lib/api';
  import { count, loaderName, titleCase } from '../lib/format';
  import { app } from '../lib/state.svelte';

  let { target }: { target?: string } = $props();

  const KINDS: { id: ContentKind; label: string }[] = [
    { id: 'mod', label: 'Mods' },
    { id: 'modpack', label: 'Modpacks' },
    { id: 'shader', label: 'Shaders' },
    { id: 'resourcepack', label: 'Resource packs' },
    { id: 'datapack', label: 'Data packs' },
  ];
  const SORTS: { id: Sort; label: string }[] = [
    { id: 'relevance', label: 'Relevance' },
    { id: 'downloads', label: 'Downloads' },
    { id: 'follows', label: 'Follows' },
    { id: 'newest', label: 'Newest' },
    { id: 'updated', label: 'Recently updated' },
  ];
  const LOADERS: LoaderKind[] = ['fabric', 'neoforge', 'forge', 'quilt'];
  const GROUP_NAMES: Record<string, string> = { categories: 'Categories', features: 'Features', resolutions: 'Resolutions', 'performance impact': 'Performance impact' };
  const PAGE_SIZE = 24;
  const SEARCH_DELAY = 240;
  /** CurseForge refuses anything past its 10,000th result. */
  const CURSEFORGE_REACH = 10000;

  let source = $state<ContentSource>('modrinth');
  let kind = $state<ContentKind>(untrack(() => app.discoverKind ?? (target ? 'mod' : 'modpack')));
  let text = $state('');
  let sort = $state<Sort>('relevance');
  let instanceId = $state(untrack(() => target ?? app.target?.id ?? ''));
  let matchInstance = $state(true);
  let minecraft = $state('');
  let loader = $state<LoaderKind | ''>('');
  let chosen = $state<string[]>([]);
  let categories = $state<SearchCategory[]>([]);
  let versions = $state<string[]>([]);
  let hits = $state<ContentHit[]>([]);
  let total = $state(0);
  let page = $state(1);
  let results = $state<HTMLDivElement>();
  let loading = $state(true);
  let hasCurseForge = $state(false);
  let installing = $state<string | null>(null);

  const instance = $derived(app.entry(instanceId));
  /** Mods and packs are filtered to the instance they'd go into, unless the player says otherwise. */
  const locked = $derived(kind !== 'modpack' && !!instance && matchInstance);
  const usesLoader = $derived(kind === 'mod' || kind === 'modpack');
  const groups = $derived.by(() => {
    const found = new Map<string, SearchCategory[]>();
    for (const category of categories) found.set(category.group, [...(found.get(category.group) ?? []), category]);
    return [...found];
  });
  const filtered = $derived(chosen.length > 0 || (!locked && (minecraft !== '' || loader !== '')));
  const pages = $derived(Math.ceil((source === 'curseforge' ? Math.min(total, CURSEFORGE_REACH) : total) / PAGE_SIZE));

  let requestId = 0;

  async function run(number: number) {
    const request = ++requestId;
    loading = true;
    try {
      const version = locked ? instance!.minecraft : minecraft || undefined;
      const modLoader = locked ? instance!.loader.kind : loader || undefined;
      const found = await api.search(source, { text: text.trim(), kind, sort, categories: chosen, minecraft: version, loader: usesLoader ? modLoader : undefined, offset: (number - 1) * PAGE_SIZE, limit: PAGE_SIZE });
      if (request !== requestId) return;
      hits = found.hits;
      total = found.total;
      page = number;
      results?.scrollTo(0, 0);
    } catch (error) {
      if (request === requestId) app.fail(error, 'Search is not available right now');
    }
    if (request === requestId) loading = false;
  }

  onMount(async () => {
    hasCurseForge = await api.curseforgeAvailable();
    const manifest = await api.minecraftVersions().catch(() => null);
    versions = manifest?.versions.filter((v) => v.kind === 'release').map((v) => v.id) ?? [];
  });

  $effect(() => {
    source;
    kind;
    text;
    sort;
    instanceId;
    matchInstance;
    minecraft;
    loader;
    chosen;
    const timer = setTimeout(() => untrack(() => run(1)), SEARCH_DELAY);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    app.contentTick;
    if (instanceId) app.loadInstalled(instanceId);
  });

  // Category ids belong to one store and one kind of content. Old results go
  // straight away too, so Install can't be pressed on a row from the last tab.
  $effect(() => {
    if ((source === 'ftb' || source === 'atlauncher') && kind !== 'modpack') source = 'modrinth';
    const list = `${source}:${kind}`;
    chosen = [];
    hits = [];
    total = 0;
    page = 1;
    const known = categoryLists.get(list);
    if (known) {
      categories = known;
      return;
    }
    categories = [];
    let current = true;
    api.searchCategories(source, kind).then(
      (found) => {
        categoryLists.set(list, found);
        if (current) categories = found;
      },
      () => {},
    );
    return () => (current = false);
  });

  function toggleCategory(id: string, on: boolean) {
    chosen = on ? [...chosen, id] : chosen.filter((other) => other !== id);
  }

  function clearFilters() {
    chosen = [];
    minecraft = '';
    loader = '';
  }

  async function install(hit: ContentHit) {
    installing = hit.id;
    await app.install(hit, instanceId);
    installing = null;
  }

  function openPage(hit: ContentHit) {
    const preview = { title: hit.title, icon: hit.icon, summary: hit.summary };
    app.openProject({ source: hit.source, id: hit.id, kind: hit.kind, instance: instanceId || undefined, preview });
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
      <label class="search-box wide"><Search size={16} /><input class="field search" placeholder="Search" aria-label="Search" bind:value={text} /></label>
      <label for="sort" class="muted">Sort</label>
      <select id="sort" class="field sort" bind:value={sort}>
        {#each SORTS as item (item.id)}<option value={item.id}>{item.label}</option>{/each}
      </select>
      <label for="source" class="muted">Source</label>
      <select id="source" class="field source" bind:value={source}>
        <option value="modrinth">Modrinth</option>
        <option value="curseforge" disabled={!hasCurseForge}>CurseForge</option>
        {#if kind === 'modpack'}
          <option value="ftb">Feed The Beast</option>
          <option value="atlauncher">ATLauncher</option>
        {/if}
      </select>
      {#if kind !== 'modpack' && app.library.length}
        <label for="target" class="muted">Install to</label>
        <select id="target" class="field target" bind:value={instanceId}>
          {#each app.library as entry (entry.id)}<option value={entry.id}>{entry.name}</option>{/each}
        </select>
      {/if}
    </div>
  </div>

  <div class="body">
    <section class="side" aria-label="Filters">
      {#if kind !== 'modpack' && instance}
        <label class="check"><input type="checkbox" bind:checked={matchInstance} /> Only for {instance.minecraft} {loaderName(instance.loader.kind)}</label>
      {/if}
      {#if !locked}
        <div class="choice">
          <label for="minecraft" class="heading">Minecraft</label>
          <select id="minecraft" class="field" bind:value={minecraft}>
            <option value="">All versions</option>
            {#each versions as version (version)}<option value={version}>{version}</option>{/each}
          </select>
        </div>
        {#if usesLoader}
          <div class="choice">
            <label for="loader" class="heading">Loader</label>
            <select id="loader" class="field" bind:value={loader}>
              <option value="">All loaders</option>
              {#each LOADERS as name (name)}<option value={name}>{loaderName(name)}</option>{/each}
            </select>
          </div>
        {/if}
      {/if}
      {#each groups as [group, list] (group)}
        <fieldset>
          <legend class="heading">{GROUP_NAMES[group] ?? titleCase(group)}</legend>
          {#each list as category (category.id)}
            <label class="check">
              <input type="checkbox" checked={chosen.includes(category.id)} onchange={(event) => toggleCategory(category.id, event.currentTarget.checked)} />
              <span class="truncate">{titleCase(category.name)}</span>
            </label>
          {/each}
        </fieldset>
      {/each}
      {#if filtered}<button class="clear" onclick={clearFilters}>Clear filters</button>{/if}
    </section>

    <div class="results" class:loading bind:this={results}>
      {#if hits.length}
        <div class="found">
          <span class="faint tabular">{total.toLocaleString()} results</span>
          <Pager {page} {pages} onpage={run} />
        </div>
      {/if}
      <ul>
        {#each hits as hit (`${hit.source}:${hit.id}`)}
          <li>
            <button class="open" onclick={() => openPage(hit)}>
              <span class="banner">{#if hit.banner}<img src={hit.banner} alt="" loading="lazy" referrerpolicy="no-referrer" />{:else}<Art seed={hit.title} width={150} height={68} />{/if}</span>
              <span class="icon">{#if hit.icon}<img src={hit.icon} alt="" loading="lazy" />{:else}{hit.title.charAt(0).toUpperCase()}{/if}</span>
              <span class="info">
                <span class="title truncate">{hit.title}</span>
                {#if hit.author}<span class="faint by truncate">by {hit.author}</span>{/if}
                <span class="muted summary">{hit.summary}</span>
              </span>
            </button>
            <div class="foot">
              <span class="faint tabular downloads">{#if hit.downloads}{count(hit.downloads)} downloads{/if}</span>
              {#if app.isInstalled(hit, instanceId)}
                <span class="installed"><Check size={15} /> Installed</span>
              {:else if hit.kind === 'datapack'}
                <button class="btn add" onclick={() => openPage(hit)}><Download size={15} /> Install</button>
              {:else}
                <button class="btn add" disabled={installing !== null || (hit.kind !== 'modpack' && !instanceId)} onclick={() => install(hit)}>
                  <Download size={15} /> {installing === hit.id ? 'Installing' : 'Install'}
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>

      {#if !loading && !hits.length}
        <p class="none muted">Nothing found.</p>
      {/if}
      {#if hits.length}
        <div class="bottom"><Pager {page} {pages} onpage={run} /></div>
      {/if}
    </div>
  </div>
</div>

<style>
  .page { display: flex; flex-direction: column; height: 100%; }
  .toolbar { flex: none; padding: 6px var(--gutter) 0; }
  .filters { display: flex; align-items: center; gap: 8px; padding: 14px 0; }
  .wide { flex: 1; min-width: 160px; }
  .sort { width: 150px; }
  .source { width: 150px; }
  .target { width: 190px; }
  .filters > label:not(.search-box) { margin-left: 6px; }

  .body { flex: 1; min-height: 0; display: grid; grid-template-columns: 196px minmax(0, 1fr); gap: 20px; padding-left: var(--gutter); }
  .side { min-height: 0; overflow-y: auto; padding: 0 6px var(--gutter) 0; }
  .heading { display: block; margin-bottom: 6px; color: var(--text); font-weight: 600; }
  .choice { margin-bottom: 14px; }
  fieldset { min-width: 0; margin: 0 0 14px; padding: 0; border: 0; }
  legend { padding: 0; }
  .check { display: flex; align-items: center; gap: 9px; min-width: 0; padding: 4px 0; color: var(--text-2); cursor: pointer; }
  .check:hover { color: var(--text); }
  .side > .check { margin-bottom: 12px; }
  .clear { color: var(--text-2); text-decoration: underline; text-underline-offset: 2px; }
  .clear:hover { color: var(--text); }

  .results { min-height: 0; overflow-y: auto; padding: 0 16px var(--gutter) 0; }
  .results.loading ul { opacity: 0.6; }
  .found { display: flex; align-items: center; justify-content: space-between; min-height: 34px; margin-bottom: 10px; }
  ul { display: grid; grid-template-columns: repeat(auto-fill, minmax(248px, 1fr)); gap: 14px; }
  li {
    --step: 3px;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--panel);
    clip-path: var(--stepped);
    content-visibility: auto;
    contain-intrinsic-size: auto 290px;
    transition: background-color 100ms ease-out;
  }
  li:hover { background: var(--raised); }
  .open { position: relative; display: flex; flex: 1; flex-direction: column; min-width: 0; text-align: left; }
  .banner { display: block; overflow: hidden; aspect-ratio: 11 / 5; background: var(--control); }
  .banner img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .icon {
    position: absolute;
    top: 0;
    left: 12px;
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    margin-top: calc((100% * 5 / 11) - 34px);
    overflow: hidden;
    background: var(--control);
    clip-path: var(--stepped);
    color: var(--text-3);
    font-size: var(--text-xl);
    font-weight: 600;
  }
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .info { display: flex; flex-direction: column; min-width: 0; padding: 26px 12px 0; }
  .title { font-size: var(--text-l); font-weight: 700; letter-spacing: -0.01em; }
  .open:hover .title { text-decoration: underline; text-underline-offset: 2px; }
  .by { font-size: var(--text-xs); }
  .summary {
    display: -webkit-box;
    overflow: hidden;
    margin-top: 5px;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
  .foot { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 58px; padding: 10px 12px 12px; }
  .downloads { font-size: var(--text-xs); }
  .installed { display: inline-flex; align-items: center; gap: 5px; height: 36px; color: var(--good); font-weight: 600; }
  .none { padding: 20px 0; }
  .bottom { display: flex; justify-content: center; padding-top: 16px; }
</style>

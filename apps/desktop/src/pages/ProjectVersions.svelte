<script lang="ts">
  import { untrack } from 'svelte';
  import { ChevronDown, ChevronRight, ExternalLink } from '@lucide/svelte';
  import RichText from '../components/RichText.svelte';
  import { api, openExternal } from '../lib/api';
  import type { LibraryEntry, ProjectPage, VersionRow } from '../lib/api';
  import { bytes, count, day, loaderName, minecraftRange, sortMinecraft } from '../lib/format';

  let {
    page,
    instance,
    matched,
    installed,
    locked,
    busy,
    onadd,
    onswitch,
    onproject,
  }: {
    page: ProjectPage;
    instance?: LibraryEntry;
    /** Whether a version fits the instance. A project can support its Minecraft version and its loader without having one that does both. */
    matched: boolean;
    /** The version in the chosen instance, if the project is already there. */
    installed?: string;
    /** True when a version can't be installed, like for a different build of the mod. */
    locked: boolean;
    busy: boolean;
    onadd: (version: string) => void;
    /** Replaces the installed file with this version. */
    onswitch: (version: string) => void;
    onproject: (slug: string) => void;
  } = $props();

  const PAGE = 60;
  const CHANNELS = ['release', 'beta', 'alpha'];

  const start = untrack(() => ({
    minecraft: matched && instance && page.gameVersions.includes(instance.minecraft) ? instance.minecraft : '',
    loader: matched && instance && page.loaders.includes(instance.loader.kind) ? instance.loader.kind : '',
  }));

  let minecraft = $state(start.minecraft);
  let loader = $state<string>(start.loader);
  let channel = $state('');
  let rows = $state<VersionRow[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let failed = $state(false);
  let limit = $state(PAGE);
  let open = $state<string | null>(null);
  let changelogs = $state<Record<string, string>>({});

  const options = $derived(sortMinecraft(page.gameVersions));
  const matching = $derived(channel ? rows.filter((row) => row.channel === channel) : rows);
  const shown = $derived(matching.slice(0, limit));
  const more = $derived(matching.length > limit || rows.length < total);

  let requestId = 0;

  async function fetchRows(offset: number) {
    const request = ++requestId;
    loading = true;
    failed = false;
    try {
      const list = await api.projectVersions(page.source, page.id, { minecraft: minecraft || undefined, loader: loader || undefined, offset });
      if (request !== requestId) return;
      rows = offset ? [...rows, ...list.versions] : list.versions;
      total = list.total;
    } catch {
      if (request === requestId) failed = true;
    }
    if (request === requestId) loading = false;
  }

  $effect(() => {
    minecraft;
    loader;
    limit = PAGE;
    open = null;
    untrack(() => fetchRows(0));
  });

  function showMore() {
    limit += PAGE;
    if (limit > rows.length && rows.length < total) fetchRows(rows.length);
  }

  async function toggle(row: VersionRow) {
    open = open === row.id ? null : row.id;
    if (open && !(row.id in changelogs)) {
      const text = await api.versionChangelog(page.source, page.id, row.id).catch(() => '');
      changelogs[row.id] = text;
    }
  }

  /** The same test the core applies when adding, so the button can't lie. */
  function fits(row: VersionRow): boolean {
    if (!instance || !row.gameVersions.includes(instance.minecraft)) return false;
    if (page.kind !== 'mod') return true;
    const accepted = instance.loader.kind === 'quilt' ? ['quilt', 'fabric'] : [instance.loader.kind];
    return row.loaders.some((name) => accepted.includes(name));
  }

  const isInstalled = (row: VersionRow) => installed !== undefined && (row.number === installed || row.name === installed);
  const siteUrl = (row: VersionRow) => {
    if (page.source === 'ftb' || page.source === 'atlauncher') return page.page;
    return page.source === 'modrinth' ? `${page.page}/version/${row.number}` : `${page.page}/files/${row.id}`;
  };
  const storeName = { modrinth: 'Modrinth', curseforge: 'CurseForge', ftb: 'Feed The Beast', atlauncher: 'ATLauncher' };
  const channelName = (name: string) => name.charAt(0).toUpperCase() + name.slice(1);
</script>

<div class="filters">
  <select class="field" aria-label="Minecraft version" bind:value={minecraft}>
    <option value="">All Minecraft versions</option>
    {#each options as version (version)}<option value={version}>{version}</option>{/each}
  </select>
  {#if page.loaders.length > 1}
    <select class="field" aria-label="Loader" bind:value={loader}>
      <option value="">All loaders</option>
      {#each page.loaders as name (name)}<option value={name}>{loaderName(name)}</option>{/each}
    </select>
  {/if}
  <select class="field" aria-label="Release type" bind:value={channel}>
    <option value="">All release types</option>
    {#each CHANNELS as name (name)}<option value={name}>{channelName(name)}</option>{/each}
  </select>
  <span class="spacer"></span>
  {#if !loading || rows.length}<span class="faint small tabular">{channel ? matching.length : total} {(channel ? matching.length : total) === 1 ? 'version' : 'versions'}</span>{/if}
</div>

{#if failed}
  <p class="muted none">The version list could not be loaded.</p>
{:else if !loading && !matching.length}
  <p class="muted none">No versions match.</p>
{/if}

<ul>
  {#each shown as row (row.id)}
    {@const expanded = open === row.id}
    <li class:expanded>
      <div class="row">
        <button class="line" aria-expanded={expanded} onclick={() => toggle(row)}>
          <span class="arrow">{#if expanded}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}</span>
          <span class="name">
            <span class="title truncate">{row.name}</span>
            <span class="faint small truncate">{row.file}</span>
          </span>
          <span class="channel {row.channel}">{channelName(row.channel)}</span>
          <span class="muted truncate">{minecraftRange(row.gameVersions)}</span>
          <span class="muted truncate">{row.loaders.map(loaderName).join(', ')}</span>
          <span class="faint tabular truncate">{day(row.published)}</span>
        </button>
        <span class="action">
          {#if isInstalled(row)}
            <span class="have">Installed</span>
          {:else if !row.available}
            <span class="faint small" title="The author doesn't allow downloads through other apps">Website only</span>
          {:else if !locked && page.kind !== 'modpack' && fits(row)}
            {#if installed === undefined}
              <button class="btn" disabled={busy} onclick={() => onadd(row.id)}>Install</button>
            {:else}
              <button class="btn" disabled={busy} onclick={() => onswitch(row.id)}>Switch</button>
            {/if}
          {/if}
        </span>
      </div>
      {#if expanded}
        <div class="detail">
          <p class="facts faint small">
            {bytes(row.size)} · {count(row.downloads)} downloads · For {row.gameVersions.join(', ')}
            <button class="site" onclick={() => openExternal(siteUrl(row))}>Open on {storeName[page.source]} <ExternalLink size={12} /></button>
          </p>
          {#if row.id in changelogs}
            {#if changelogs[row.id].trim()}
              <RichText source={changelogs[row.id]} format={page.format} base={page.page} {onproject} />
            {:else}
              <p class="muted">No changelog for this version.</p>
            {/if}
          {:else}
            <p class="muted">Loading the changelog</p>
          {/if}
        </div>
      {/if}
    </li>
  {/each}
</ul>

{#if loading && !rows.length}
  <p class="muted none">Loading versions</p>
{:else if more}
  <div class="more"><button class="btn" disabled={loading} onclick={showMore}>{loading ? 'Loading' : 'Show more'}</button></div>
{/if}

<style>
  .filters { display: flex; align-items: center; gap: 8px; margin-bottom: 12px; }
  .filters .field { width: 190px; }
  .spacer { flex: 1; }
  .none { padding: 14px 0; }
  ul { box-shadow: inset 0 1px var(--line); }
  li { box-shadow: inset 0 -1px var(--line); content-visibility: auto; contain-intrinsic-size: auto 60px; }
  .row { display: grid; grid-template-columns: minmax(0, 1fr) 96px; align-items: center; transition: background-color 80ms ease-out; }
  .row:hover { background: var(--selected); }
  .line {
    display: grid;
    grid-template-columns: 16px minmax(0, 3fr) 58px minmax(104px, 1fr) minmax(84px, 1fr) 92px;
    gap: 10px;
    align-items: center;
    min-width: 0;
    height: 60px;
    padding-left: 4px;
    text-align: left;
  }
  .arrow { color: var(--text-3); }
  .name { display: flex; flex-direction: column; min-width: 0; }
  .title { font-weight: 600; }
  .channel { color: var(--text-2); font-weight: 500; }
  .channel.beta { color: var(--warn); }
  .channel.alpha { color: var(--bad); }
  .action { display: flex; justify-content: flex-end; padding-right: 6px; text-align: right; }
  .have { color: var(--good); font-weight: 600; }
  .detail { padding: 4px 16px 16px 40px; }
  .facts { display: flex; align-items: center; gap: 6px; margin-bottom: 10px; }
  .site { display: inline-flex; align-items: center; gap: 4px; margin-left: auto; color: var(--text-2); }
  .site:hover { color: var(--text); }
  .more { padding: 14px 0; text-align: center; }
</style>

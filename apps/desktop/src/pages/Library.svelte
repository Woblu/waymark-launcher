<script lang="ts">
  import { ChevronDown, ChevronRight, Play, Plus, Search } from '@lucide/svelte';
  import Art from '../components/Art.svelte';
  import { api, instanceIcon, localImage } from '../lib/api';
  import type { LibraryEntry, RecentWorld } from '../lib/api';
  import { loaderName, playtime, timeAgo } from '../lib/format';
  import { instanceMenu } from '../lib/instance-menu';
  import { app } from '../lib/state.svelte';
  import Instance from './Instance.svelte';

  const FOLDED_KEY = 'waymark.folded-folders';

  function loadFolded(): string[] {
    try {
      return JSON.parse(localStorage.getItem(FOLDED_KEY) ?? '[]');
    } catch {
      return [];
    }
  }

  let filter = $state('');
  let recent = $state<RecentWorld[]>([]);
  let folded = $state(loadFolded());

  function fold(group: string) {
    folded = folded.includes(group) ? folded.filter((name) => name !== group) : [...folded, group];
    try {
      localStorage.setItem(FOLDED_KEY, JSON.stringify(folded));
    } catch {
      // Only remembers what's folded, so it's fine to lose.
    }
  }

  $effect(() => {
    app.contentTick;
    app.library;
    api.recentWorlds().then((found) => (recent = found), () => {});
  });

  const shown = $derived.by(() => {
    const text = filter.trim().toLowerCase();
    return text ? app.library.filter((entry) => entry.name.toLowerCase().includes(text)) : app.library;
  });
  const current = $derived(app.current);
  const latest = $derived(app.focused);
  const groups = $derived.by(() => {
    const found = new Map<string, LibraryEntry[]>();
    for (const entry of shown) found.set(entry.group ?? '', [...(found.get(entry.group ?? '') ?? []), entry]);
    return [...found].sort(([a], [b]) => (a && b ? a.localeCompare(b) : a ? -1 : 1));
  });

  function openMenu(event: MouseEvent, entry: LibraryEntry) {
    event.preventDefault();
    app.menu = { x: event.clientX, y: event.clientY, items: instanceMenu(entry) };
  }
</script>

{#snippet card(entry: LibraryEntry)}
  {@const picture = instanceIcon(entry, app.iconTick)}
  <li class="card">
    <button class="open" data-instance={entry.id} oncontextmenu={(event) => openMenu(event, entry)} onclick={() => app.open(entry.id, app.tab)}>
      <Art seed={entry.name} width={200} height={125} />
      {#if picture}<img class="mark" src={picture} alt="" />{/if}
      <span class="label">
        <span class="name truncate">{entry.name}</span>
        <span class="sub truncate">
          {#if entry.running}<span class="running">Running</span>{:else}{loaderName(entry.loader.kind)} {entry.minecraft}{/if}
        </span>
      </span>
    </button>
    {#if !entry.running}
      <button class="btn primary icon quick" aria-label="Play {entry.name}" title="Play" onclick={() => app.play(entry.id)}><Play size={15} fill="currentColor" /></button>
    {/if}
  </li>
{/snippet}

{#if current}
  {#key current.id}
    <!-- Not current.id, because a tab still loading reads this after a delete clears it. -->
    <Instance id={app.selected!} />
  {/key}
{:else}
  <div class="page">
    <div class="scene hero">
      <Art seed={latest?.name ?? 'Waymark'} width={480} height={130} />
      <div class="over">
        {#if latest}
          <h1 class="name">{latest.name}</h1>
          <p class="facts">
            {loaderName(latest.loader.kind)} {latest.minecraft}
            {#if latest.playtimeSecs >= 60}<span>{playtime(latest.playtimeSecs)}</span>{/if}
            <span>{latest.lastPlayed ? `Last played ${timeAgo(latest.lastPlayed)}` : 'Not played yet'}</span>
          </p>
        {:else if app.loaded}
          <h1>No instances yet</h1>
          <p class="facts">Create an instance, or import one from another launcher. Importing leaves the original alone.</p>
        {/if}
      </div>
    </div>

    <div class="content">
      {#if recent.length}
        <section>
          <h2>Jump back in</h2>
          <ul class="recent">
            {#each recent as world (world.instance + world.folder)}
              <li>
                <button class="world" disabled={app.entry(world.instance)?.running} title="Play {world.name}" onclick={() => app.play(world.instance, { kind: 'world', folder: world.folder })}>
                  <span class="thumb">{#if world.hasIcon}<img src={localImage(`world/${world.instance}/${world.folder}`)} alt="" />{:else}{world.name.charAt(0).toUpperCase()}{/if}</span>
                  <span class="text">
                    <span class="name truncate">{world.name}</span>
                    <span class="sub truncate">{world.instanceName}</span>
                  </span>
                  <Play size={15} fill="currentColor" />
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      <section>
        <div class="head">
          <h2>Instances</h2>
          {#if app.library.length}
            <label class="search-box filter"><Search size={16} /><input class="field" placeholder="Search instances" aria-label="Search instances" bind:value={filter} /></label>
          {/if}
          <span class="spacer"></span>
          {#if !app.library.length && app.loaded}<button class="btn" onclick={() => app.go('discover')}>Browse modpacks</button>{/if}
          <button class="btn primary" onclick={() => (app.dialog = 'new')}><Plus size={16} /> New instance</button>
        </div>

        {#each groups as [group, entries] (group)}
          {#if group}
            <button class="folder" aria-expanded={!folded.includes(group)} onclick={() => fold(group)}>
              {#if folded.includes(group)}<ChevronRight size={16} />{:else}<ChevronDown size={16} />{/if}
              <span class="truncate">{group}</span>
              <span class="faint">{entries.length}</span>
            </button>
          {/if}
          {#if !folded.includes(group)}
            <ul class="grid">
              {#each entries as entry (entry.id)}{@render card(entry)}{/each}
            </ul>
          {/if}
        {/each}
      </section>
    </div>
  </div>
{/if}

<style>
  .page { height: 100%; overflow-y: auto; }
  .hero { height: clamp(170px, 30vh, 280px); }
  .over { position: absolute; right: var(--gutter); bottom: 14px; left: var(--gutter); z-index: 1; }
  .over h1 { font-size: 34px; letter-spacing: -0.025em; }
  .facts { margin-top: 4px; color: var(--text-2); }
  .facts span::before { content: ''; display: inline-block; width: 4px; height: 4px; margin: 0 8px 2px; background: var(--text-3); }

  .content { padding: 14px var(--gutter) var(--gutter); }
  section + section { margin-top: 26px; }
  h2 { font-size: var(--text-l); }
  section > h2 { margin-bottom: 10px; }

  .recent { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px; }
  .world { display: flex; align-items: center; gap: 12px; width: 100%; padding: 8px 14px 8px 8px; background: var(--panel); clip-path: var(--stepped); color: var(--text-3); text-align: left; transition: background-color 100ms ease-out, color 100ms ease-out; }
  .world:hover:not(:disabled) { background: var(--raised); color: var(--accent); }
  .world:disabled { opacity: 0.5; cursor: not-allowed; }
  .thumb { flex: none; display: grid; place-items: center; width: 40px; height: 40px; overflow: hidden; background: var(--control); color: var(--text-3); font-size: var(--text-l); font-weight: 600; image-rendering: pixelated; }
  .thumb img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .text { flex: 1; display: flex; flex-direction: column; min-width: 0; color: var(--text); }

  .head { display: flex; align-items: center; gap: 12px; margin-bottom: 12px; }
  .filter { flex: 0 1 280px; }
  .spacer { flex: 1; }
  .folder { display: flex; align-items: center; gap: 6px; margin: 18px 0 10px; color: var(--text-2); font-weight: 600; }
  .folder:hover { color: var(--text); }

  /* Each instance is a postcard of its own place. */
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(236px, 1fr)); gap: 12px; }
  .card { --step: 3px; position: relative; min-width: 0; aspect-ratio: 16 / 10; clip-path: var(--stepped); }
  .open { position: relative; display: block; overflow: hidden; width: 100%; height: 100%; text-align: left; }
  .open::after { content: ''; position: absolute; inset: 0; background: linear-gradient(rgb(8 12 18 / 0%) 35%, rgb(8 12 18 / 92%)); pointer-events: none; }
  .mark { position: absolute; top: 10px; left: 10px; z-index: 1; width: 36px; height: 36px; clip-path: var(--stepped); object-fit: cover; }
  .label { position: absolute; right: 12px; bottom: 9px; left: 12px; z-index: 1; display: flex; flex-direction: column; min-width: 0; }
  .name { font-size: var(--text-m); font-weight: 650; }
  .label .name { color: #f0ebe1; }
  .sub { color: var(--text-3); font-size: var(--text-xs); }
  .label .sub { color: #b4bcc8; }
  .running { color: var(--good); font-weight: 600; }
  .card:hover .name { text-decoration: underline; text-underline-offset: 2px; }
  .quick { position: absolute; right: 10px; bottom: 10px; z-index: 2; opacity: 0; transition: opacity 100ms ease-out, background-color 100ms ease-out; }
  .card:hover .quick, .quick:focus-visible { opacity: 1; }
</style>

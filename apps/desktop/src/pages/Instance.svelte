<script lang="ts">
  import { Ellipsis, FolderOpen, Plus } from '@lucide/svelte';
  import Art from '../components/Art.svelte';
  import ManualFiles from '../components/ManualFiles.svelte';
  import { api, instanceIcon, openFolder } from '../lib/api';
  import type { BisectView, ContentKind, ManualFile, PackUpdate, Verdict } from '../lib/api';
  import { loaderName, playtime, timeAgo } from '../lib/format';
  import { instanceMenu } from '../lib/instance-menu';
  import { app } from '../lib/state.svelte';
  import type { InstanceTab } from '../lib/state.svelte';
  import { tabKeys } from '../lib/tabs';
  import InstanceSettings from './InstanceSettings.svelte';
  import LogsTab from './LogsTab.svelte';
  import ModsTab from './ModsTab.svelte';
  import PacksTab from './PacksTab.svelte';
  import ServerTab from './ServerTab.svelte';
  import TrailTab from './TrailTab.svelte';
  import WorldsTab from './WorldsTab.svelte';

  let { id }: { id: string } = $props();

  const TABS: { id: InstanceTab; label: string }[] = [
    { id: 'mods', label: 'Mods' },
    { id: 'resourcepacks', label: 'Resource packs' },
    { id: 'shaders', label: 'Shaders' },
    { id: 'worlds', label: 'Worlds' },
    { id: 'trail', label: 'Waymarks' },
    { id: 'logs', label: 'Logs' },
    { id: 'server', label: 'Server' },
  ];

  /** These only work with a loader, unless an older instance already has some. */
  const CONTENT_TABS: { id: InstanceTab; kind: ContentKind }[] = [
    { id: 'mods', kind: 'mod' },
    { id: 'shaders', kind: 'shader' },
  ];

  const entry = $derived(app.entry(id));
  const vanilla = $derived(entry?.loader.kind === 'vanilla');
  let filled = $state<InstanceTab[]>([]);
  const tabs = $derived(TABS.filter((item) => !vanilla || !CONTENT_TABS.some((tab) => tab.id === item.id) || filled.includes(item.id)));
  const picture = $derived(entry ? instanceIcon(entry, app.iconTick) : undefined);

  let search = $state<BisectView | null>(null);
  let packUpdate = $state<PackUpdate | null>(null);
  let updatingPack = $state(false);
  let manual = $state<ManualFile[]>([]);
  let showManual = $state(false);

  $effect(() => {
    app.contentTick;
    // A finished search has no status on disk, so keep its result on screen
    // until the player dismisses it.
    api.bisectStatus(id).then((view) => {
      if (view || search?.status === 'testing') search = view;
    });
  });

  $effect(() => {
    app.contentTick;
    api.checkPackUpdate(id).then((found) => (packUpdate = found)).catch(() => {});
    api.manualFiles(id).then((files) => (manual = files)).catch(() => {});
  });

  $effect(() => {
    app.contentTick;
    if (!vanilla) return;
    let current = true;
    Promise.all(CONTENT_TABS.map((tab) => api.content(id, tab.kind).catch(() => []))).then((found) => {
      if (current) filled = CONTENT_TABS.filter((_, index) => found[index].length).map((tab) => tab.id);
    });
    return () => (current = false);
  });

  // Settings has no tab of its own. It opens from the More actions menu.
  $effect(() => {
    if (entry && app.tab !== 'settings' && !tabs.some((item) => item.id === app.tab)) app.tab = 'worlds';
  });

  function openMore(event: MouseEvent) {
    if (!entry) return;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    app.menu = { x: box.left, y: box.bottom + 4, items: instanceMenu(entry) };
  }

  async function updatePack() {
    updatingPack = true;
    try {
      const report = await api.updatePack(id);
      packUpdate = null;
      const notes = [
        report.kept.length ? `Kept ${report.kept.length} of your changes.` : '',
        report.stayedRemoved.length ? `${report.stayedRemoved.length} files you removed stayed out.` : '',
        report.minecraftChanged ? 'Minecraft changed version, so check that your own mods still fit.' : '',
        'You can undo this from Waymarks.',
      ];
      // One sentence a line, so each is translated on its own.
      app.changed(id, `Updated to ${report.version}`, notes.filter(Boolean).join('\n'));
      app.contentTick++;
      app.checkUpdate(id);
      await app.refresh();
    } catch (error) {
      app.fail(error, 'Could not update the pack');
    }
    updatingPack = false;
  }

  async function report(verdict: Verdict) {
    try {
      search = await api.reportBisect(id, verdict);
      app.contentTick++;
      await app.refresh();
    } catch (error) {
      app.fail(error);
    }
  }

  async function stopSearch() {
    try {
      await api.cancelBisect(id);
      search = null;
      app.contentTick++;
      await app.refresh();
    } catch (error) {
      app.fail(error);
    }
  }

  async function openGameFolder() {
    await openFolder(await api.gameDirectory(id));
  }
</script>

{#if entry}
  <div class="page">
    <div class="scene top">
      <Art seed={entry.name} width={480} height={100} />
      <div class="title">
        {#if picture}<img class="picture" src={picture} alt="" />{/if}
        <div class="names">
          <h1 class="truncate">{entry.name}</h1>
          <p class="facts">
            {loaderName(entry.loader.kind)} {entry.minecraft}{entry.loader.version ? `, ${entry.loader.version}` : ''}
            {#if entry.playtimeSecs >= 60}<span>{playtime(entry.playtimeSecs)}</span>{/if}
            <span>{entry.lastPlayed ? `Last played ${timeAgo(entry.lastPlayed)}` : 'Not played yet'}</span>
            {#if entry.server}<span>{`Joins ${entry.server.name}`}</span>{/if}
          </p>
        </div>
        <button class="btn" onclick={() => app.go('discover', id, vanilla ? 'resourcepack' : undefined)}><Plus size={16} /> Install content</button>
        <button class="btn icon" aria-label="Open folder" title="Open folder" onclick={openGameFolder}><FolderOpen size={17} /></button>
        <button class="btn icon" aria-label="More actions" title="More actions" onclick={openMore}><Ellipsis size={17} /></button>
      </div>
    </div>

    {#if search?.status === 'testing'}
      <div class="notice">
        <span><strong>Crash search, test {search.round} of about {search.estimate}.</strong> {search.enabled} of {search.total} mods are on. Launch the game and report what happens.</span>
        <button class="btn" onclick={stopSearch}>Stop search</button>
        <button class="btn" onclick={() => report('works')}>It worked</button>
        <button class="btn" onclick={() => report('crashes')}>It crashed</button>
      </div>
    {:else if search?.status === 'found'}
      <div class="notice">
        <span>
          <strong>{`Found: ${search.culprits.join(', ')}.`}</strong>
          {search.culprits.length === 1 ? 'It has been switched off and your other mods are back on.' : 'They have been switched off and your other mods are back on.'}
        </span>
        <button class="btn" onclick={() => (search = null)}>Done</button>
      </div>
    {:else if search?.status === 'notAMod'}
      <div class="notice">
        <span><strong>A mod isn't the cause.</strong> The game crashes with every mod off. Your mods are back as they were.</span>
        <button class="btn" onclick={() => (search = null)}>Done</button>
      </div>
    {/if}

    {#if manual.length}
      <div class="notice">
        <span>
          <strong>{manual.length === 1 ? '1 file needs downloading by hand.' : `${manual.length} files need downloading by hand.`}</strong>
          {manual.length === 1 ? 'Its author only allows downloads from CurseForge.' : 'Their authors only allow downloads from CurseForge.'}
        </span>
        <button class="btn" onclick={() => (showManual = true)}>{manual.length === 1 ? 'Get it' : 'Get them'}</button>
      </div>
    {/if}

    {#if packUpdate}
      <div class="notice">
        <span><strong>{`${packUpdate.name} ${packUpdate.latest} is available.`}</strong> {`You're on ${packUpdate.current}. Your own mods and edited files are kept.`}</span>
        <button class="btn" disabled={updatingPack} onclick={updatePack}>{updatingPack ? 'Updating' : 'Update pack'}</button>
      </div>
    {/if}

    <div class="sections">
      <div class="tabs" role="tablist" aria-label="Instance sections" use:tabKeys>
        <!-- Settings has no tab, so while it's open the first tab is the one Tab reaches. -->
        {#each tabs as item, index (item.id)}
          {@const reachable = app.tab === item.id || (index === 0 && !tabs.some((other) => other.id === app.tab))}
          <button role="tab" aria-selected={app.tab === item.id} tabindex={reachable ? 0 : -1} class:active={app.tab === item.id} onclick={() => (app.tab = item.id)}>{item.label}</button>
        {/each}
      </div>
    </div>

    <section class="panel">
      {#if app.tab === 'mods'}
        <ModsTab {id} />
      {:else if app.tab === 'resourcepacks'}
        {#key id}<PacksTab {id} kind="resourcepack" shared={entry.shared?.includes('resourcepacks') ?? false} />{/key}
      {:else if app.tab === 'shaders'}
        {#key id}<PacksTab {id} kind="shader" shared={entry.shared?.includes('shaderpacks') ?? false} />{/key}
      {:else if app.tab === 'worlds'}
        <WorldsTab {id} running={entry.running} />
      {:else if app.tab === 'trail'}
        <TrailTab {id} />
      {:else if app.tab === 'logs'}
        <LogsTab {id} />
      {:else if app.tab === 'server'}
        <ServerTab {id} />
      {:else}
        <InstanceSettings {entry} />
      {/if}
    </section>
  </div>
  {#if showManual && manual.length}
    <ManualFiles {id} files={manual} onchange={(left) => (manual = left)} onclose={() => (showManual = false)} />
  {/if}
{/if}

<style>
  .page { display: flex; flex-direction: column; height: 100%; }
  .top { flex: none; height: clamp(120px, 21vh, 170px); }
  .title { position: absolute; right: var(--gutter); bottom: 10px; left: var(--gutter); z-index: 1; display: flex; align-items: flex-end; gap: 8px; }
  .picture { flex: none; width: 56px; height: 56px; margin-right: 6px; clip-path: var(--stepped); object-fit: cover; }
  .names { flex: 1; min-width: 0; }
  h1 { font-size: 30px; letter-spacing: -0.025em; }
  .facts { margin-top: 3px; color: var(--text-2); white-space: nowrap; }
  .facts span::before { content: ''; display: inline-block; width: 4px; height: 4px; margin: 0 8px 2px; background: var(--text-3); }
  .notice { flex: none; margin: 10px var(--gutter) 0; }
  .sections { flex: none; padding: 0 var(--gutter); }
  .panel { flex: 1; min-height: 0; }
</style>

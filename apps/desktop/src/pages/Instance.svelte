<script lang="ts">
  import Art from '../components/Art.svelte';
  import PlayButton from '../components/PlayButton.svelte';
  import { api, openFolder } from '../lib/api';
  import type { BisectView, PackUpdate, Verdict } from '../lib/api';
  import { loaderName, playtime, timeAgo } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import type { InstanceTab } from '../lib/state.svelte';
  import InstanceSettings from './InstanceSettings.svelte';
  import LogsTab from './LogsTab.svelte';
  import ModsTab from './ModsTab.svelte';
  import TrailTab from './TrailTab.svelte';

  let { id }: { id: string } = $props();

  const TABS: { id: InstanceTab; label: string }[] = [
    { id: 'mods', label: 'Mods' },
    { id: 'trail', label: 'Waymarks' },
    { id: 'logs', label: 'Logs' },
    { id: 'settings', label: 'Settings' },
  ];

  const entry = $derived(app.entry(id));
  const stats = $derived(
    entry
      ? [entry.playtimeSecs >= 60 ? playtime(entry.playtimeSecs) : '', entry.lastPlayed ? `Last played ${timeAgo(entry.lastPlayed)}` : 'Not played yet'].filter(Boolean).join('. ')
      : '',
  );

  let search = $state<BisectView | null>(null);
  let packUpdate = $state<PackUpdate | null>(null);
  let updatingPack = $state(false);

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
  });

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
      app.toast('good', `Updated to ${report.version}`, notes.filter(Boolean).join(' '));
      for (const file of report.manual) app.toast('info', `${file.name} needs a manual download`, file.page);
      app.contentTick++;
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
    <div class="banner"><Art seed={entry.name} /></div>

    <div class="title">
      <div class="names">
        <h1 class="truncate">{entry.name}</h1>
        <p class="muted truncate">
          Minecraft {entry.minecraft}, {loaderName(entry.loader.kind)}{entry.loader.version ? ` ${entry.loader.version}` : ''}. {stats}.
        </p>
      </div>
      <button class="btn" onclick={openGameFolder}>Open folder</button>
      <button class="btn" onclick={() => app.go('discover', id)}>Add content</button>
      <PlayButton {entry} />
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
        <span><strong>Found: {search.culprits.join(' and ')}.</strong> {search.culprits.length === 1 ? 'It has' : 'They have'} been switched off and your other mods are back on.</span>
        <button class="btn" onclick={() => (search = null)}>Done</button>
      </div>
    {:else if search?.status === 'notAMod'}
      <div class="notice">
        <span><strong>The game crashes with every mod off,</strong> so a mod isn't the cause. Your mods are back as they were.</span>
        <button class="btn" onclick={() => (search = null)}>Done</button>
      </div>
    {/if}

    {#if packUpdate}
      <div class="notice">
        <span><strong>{packUpdate.name} {packUpdate.latest} is available.</strong> You're on {packUpdate.current}. Your own mods and edited files are kept.</span>
        <button class="btn" disabled={updatingPack} onclick={updatePack}>{updatingPack ? 'Updating' : 'Update pack'}</button>
      </div>
    {/if}

    <nav class="tabs" aria-label="Instance sections">
      {#each TABS as item (item.id)}
        <button class:active={app.tab === item.id} onclick={() => (app.tab = item.id)}>{item.label}</button>
      {/each}
    </nav>

    <section class="panel">
      {#if app.tab === 'mods'}
        <ModsTab {id} />
      {:else if app.tab === 'trail'}
        <TrailTab {id} />
      {:else if app.tab === 'logs'}
        <LogsTab {id} />
      {:else}
        <InstanceSettings {entry} />
      {/if}
    </section>
  </div>
{/if}

<style>
  .page { display: flex; flex-direction: column; height: 100%; }
  .banner { flex: none; height: 150px; }
  .title { flex: none; display: flex; align-items: center; gap: 8px; padding: 8px 24px 12px; }
  .names { flex: 1; min-width: 0; }
  h1 { font-size: var(--text-xl); line-height: 1.2; letter-spacing: -0.015em; }
  .notice {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 24px 10px;
    padding: 8px 8px 8px 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--raised);
  }
  .notice span { flex: 1; min-width: 0; }
  .notice strong { font-weight: 600; }
  .tabs { flex: none; padding: 0 24px; }
  .panel { flex: 1; min-height: 0; }
</style>

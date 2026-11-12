<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { api, inTauri, pickPackFile } from '../lib/api';
  import type { ExternalInstance, GameVersion, LoaderKind, Starter } from '../lib/api';
  import { explain } from '../lib/errors';
  import { day, loaderName } from '../lib/format';
  import { externalVersion, inLibrary as alreadyIn, LAUNCHERS } from '../lib/imports';
  import { app, starterFits, startersFor } from '../lib/state.svelte';
  import { tabKeys } from '../lib/tabs';
  import Modal from './Modal.svelte';

  /** `importEverything` starts importing all of them as soon as they're found. */
  let { start = 'new', importEverything = false }: { start?: 'new' | 'import'; importEverything?: boolean } = $props();

  const LOADERS: LoaderKind[] = ['vanilla', 'fabric', 'neoforge', 'forge', 'quilt'];
  const OLD_LOADERS: LoaderKind[] = ['vanilla', 'forge', 'legacyfabric', 'ornithe', 'liteloader', 'rift', 'cleanroom'];
  const ERAS = [
    { id: 'release', label: 'Releases' },
    { id: 'snapshot', label: 'Snapshots' },
    { id: 'beta', label: 'Beta' },
    { id: 'alpha', label: 'Alpha' },
    { id: 'infdev', label: 'Infdev' },
    { id: 'indev', label: 'Indev' },
    { id: 'classic', label: 'Classic' },
    { id: 'preclassic', label: 'Pre-Classic' },
    { id: 'aprilfools', label: 'April Fools' },
  ];
  /** Fabric only reaches back to 1.14, and these projects carry it further. */
  const OLD_MINOR = 14;

  type ImportState = { status: 'importing' } | { status: 'done'; id: string } | { status: 'failed'; reason: string };

  let mode = $state<'new' | 'import'>(untrack(() => start));
  /** What the player typed. Until they type, the name follows the version and loader. */
  let name = $state('');
  let named = $state(false);
  let minecraft = $state('');
  let loader = $state<LoaderKind>('fabric');
  let era = $state('release');
  let versions = $state<GameVersion[]>([]);
  let external = $state<ExternalInstance[]>([]);
  let searched = $state(false);
  let code = $state('');
  let missing = $state<LoaderKind[]>([]);
  let lists = $state<Record<string, Starter[]>>({});
  let useStarters = $state(true);
  /** Snapshots are listed when Settings says so, or when the player ticks the box here. */
  let snapshots = $state(false);
  let imports = $state<Record<string, ImportState>>({});
  let importingAll = $state<{ done: number; total: number } | null>(null);

  const eras = $derived(ERAS.filter((item) => item.id !== 'snapshot' || snapshots));
  const shown = $derived(versions.filter((v) => v.era === era));
  const picked = $derived(versions.find((v) => v.id === minecraft));
  const candidates = $derived.by((): LoaderKind[] => {
    if (minecraft === 'b1.7.3') return ['vanilla', 'babric', 'ornithe'];
    if (picked && picked.era !== 'release' && picked.era !== 'snapshot') return ['vanilla'];
    const minor = /^1\.(\d+)/.exec(minecraft)?.[1];
    return minor && Number(minor) < OLD_MINOR ? OLD_LOADERS : LOADERS;
  });
  const loaders = $derived(candidates.filter((kind) => !missing.includes(kind)));
  /** Starters that could go into this instance. Each one's store is still asked when it's made. */
  const starters = $derived(startersFor(lists, minecraft).filter((item) => starterFits(item.kind, loader)));
  /** A name from what was picked, like `Fabric 1.21.4`, numbered if the library already has one. */
  const suggested = $derived.by(() => {
    if (!minecraft) return '';
    const base = `${loaderName(loader)} ${minecraft}`;
    const taken = new Set(app.library.map((entry) => entry.name));
    let candidate = base;
    for (let number = 2; taken.has(candidate); number++) candidate = `${base} (${number})`;
    return candidate;
  });
  const finalName = $derived((named ? name.trim() : '') || suggested);

  const inLibrary = (instance: ExternalInstance) => alreadyIn(instance, app.library);
  const waiting = $derived(external.filter((instance) => !imports[instance.gameDir] || imports[instance.gameDir].status === 'failed').filter((instance) => !inLibrary(instance) || imports[instance.gameDir]));

  $effect(() => {
    if (!loaders.includes(loader)) loader = 'vanilla';
  });

  $effect(() => {
    if (!snapshots && era === 'snapshot') untrack(() => chooseEra('release'));
  });

  // Each loader says for itself whether it has a build for this version. A failed
  // check keeps the loader listed, and creating the instance then says what's wrong.
  $effect(() => {
    const version = minecraft;
    const asking = candidates.filter((kind) => kind !== 'vanilla');
    let current = true;
    missing = [];
    if (!version) return;
    Promise.all(asking.map((kind) => api.loaderVersions(kind, version).then((list) => list.length === 0, () => false))).then((empty) => {
      if (current) missing = asking.filter((_, index) => empty[index]);
    });
    return () => (current = false);
  });

  function chooseEra(chosen: string) {
    era = chosen;
    minecraft = shown[0]?.id ?? '';
  }

  function typeName(event: Event & { currentTarget: HTMLInputElement }) {
    name = event.currentTarget.value;
    named = true;
  }

  function close() {
    app.dialog = 'none';
  }

  onMount(async () => {
    const [manifest, found, settings] = await Promise.allSettled([api.minecraftVersions(), api.externalInstances(), api.settings()]);
    if (settings.status === 'fulfilled') {
      lists = settings.value.starters ?? {};
      snapshots = settings.value.showSnapshots ?? false;
    }
    if (manifest.status === 'fulfilled') {
      versions = manifest.value.versions;
      minecraft = manifest.value.latest;
    } else app.fail(manifest.reason, 'Could not load Minecraft versions');
    if (found.status === 'fulfilled') external = found.value;
    searched = true;
    if (untrack(() => importEverything)) importAll();
  });

  async function create(event: SubmitEvent) {
    event.preventDefault();
    // Read before closing, which takes this dialog away.
    const withStarters = useStarters && starters.length > 0;
    const chosenName = finalName;
    close();
    try {
      const instance = await api.createInstance({ name: chosenName, minecraft, loader });
      await app.refresh();
      app.open(instance.id);
      if (withStarters) await app.addStarters(instance.id);
    } catch (error) {
      app.fail(error, 'Could not create the instance');
    }
  }

  /** Imports one and marks it, leaving the dialog open for the rest. */
  async function importOne(instance: ExternalInstance, quiet = false): Promise<boolean> {
    imports[instance.gameDir] = { status: 'importing' };
    try {
      const created = await api.importExternal(instance);
      imports[instance.gameDir] = { status: 'done', id: created.id };
      await app.refresh().catch(() => {});
      return true;
    } catch (error) {
      const explained = explain(error);
      imports[instance.gameDir] = { status: 'failed', reason: explained.sentence || explained.technical };
      if (!quiet) app.fail(error, 'Could not import that instance');
      return false;
    }
  }

  /** One after another, since each one copies a whole game folder. */
  async function importAll() {
    const queue = [...waiting];
    if (!queue.length) return;
    importingAll = { done: 0, total: queue.length };
    const failed: string[] = [];
    for (const instance of queue) {
      if (!(await importOne(instance, true))) failed.push(instance.name);
      importingAll = { done: importingAll.done + 1, total: queue.length };
    }
    importingAll = null;
    const worked = queue.length - failed.length;
    if (!failed.length) app.toast('good', worked === 1 ? 'Imported 1 instance' : `Imported ${worked} instances`);
    else app.toast('bad', `Imported ${worked} of ${queue.length} instances`, `Could not import ${failed.join(', ')}`);
  }

  function openImported(id: string) {
    close();
    app.open(id);
  }

  async function importCode(event: SubmitEvent) {
    event.preventDefault();
    close();
    try {
      const result = await api.installShare(code.trim());
      await app.refresh();
      app.open(result.instance.id);
      if (result.failed.length) app.toast('info', `${result.failed.length} could not be installed`, result.failed.slice(0, 4).join(', '));
    } catch (error) {
      app.fail(error, 'Could not use that code');
    }
  }

  async function importFile() {
    const path = await pickPackFile();
    if (!path) return;
    close();
    try {
      await app.packInstalled(await api.installPackFile(path));
    } catch (error) {
      app.fail(error, 'Could not install that pack');
    }
  }
</script>

<Modal title="Add an instance" width={540} onclose={close}>
  <div class="tabs picker" role="tablist" aria-label="Add an instance" use:tabKeys>
    <button id="add-new" role="tab" aria-selected={mode === 'new'} aria-controls="add-panel" tabindex={mode === 'new' ? 0 : -1} class:active={mode === 'new'} onclick={() => (mode = 'new')}>New</button>
    <button id="add-import" role="tab" aria-selected={mode === 'import'} aria-controls="add-panel" tabindex={mode === 'import' ? 0 : -1} class:active={mode === 'import'} onclick={() => (mode = 'import')}>
      {external.length ? `Import (${external.length})` : 'Import'}
    </button>
  </div>

  <div id="add-panel" role="tabpanel" aria-labelledby={mode === 'new' ? 'add-new' : 'add-import'}>
    {#if mode === 'new'}
      <form onsubmit={create}>
        <div class="form-row">
          <label for="name">Name</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input id="name" class="field" value={named ? name : suggested} placeholder={suggested} oninput={typeName} autofocus />
        </div>
        <div class="form-row">
          <label for="era">Era</label>
          <div>
            <select id="era" class="field" value={era} onchange={(event) => chooseEra(event.currentTarget.value)}>
              {#each eras as item (item.id)}<option value={item.id}>{item.label}</option>{/each}
            </select>
            <label class="check small-check"><input type="checkbox" bind:checked={snapshots} /> Show snapshots</label>
          </div>
        </div>
        <div class="form-row">
          <label for="version">Minecraft version</label>
          <div>
            <select id="version" class="field" bind:value={minecraft}>
              {#each shown as version (version.id)}<option value={version.id}>{version.id}</option>{/each}
            </select>
            {#if picked}<p class="faint small released">Released {day(picked.released)}</p>{/if}
          </div>
        </div>
        <div class="form-row">
          <label for="loader">Mod loader</label>
          <select id="loader" class="field" bind:value={loader}>
            {#each loaders as kind (kind)}<option value={kind}>{loaderName(kind)}</option>{/each}
          </select>
        </div>
        {#if starters.length}
          <div class="form-row">
            <span class="form-label">Starter mods</span>
            <div>
              <label class="check"><input type="checkbox" bind:checked={useStarters} /> Add starter mods ({starters.length})</label>
              <p class="faint small names truncate">{starters.map((item) => item.title).join(', ')}</p>
            </div>
          </div>
        {/if}
        {#if loader === 'cleanroom'}
          <p class="muted note">Cleanroom warns on its first launch that two patch mods, Fugue and Scalar, are missing. Both can be installed from Discover.</p>
        {/if}
        <div class="actions">
          <button type="button" class="btn" onclick={close}>Cancel</button>
          <button class="btn primary" disabled={!finalName || !minecraft}>Create</button>
        </div>
      </form>
    {:else}
      {#if external.length}
        <div class="found">
          <p class="muted">Found on this computer. Importing makes a copy and leaves the original alone.</p>
          {#if waiting.length > 1 || importingAll}
            <button class="btn primary" disabled={!!importingAll} onclick={importAll}>
              {importingAll ? `Importing ${importingAll.done + 1} of ${importingAll.total}` : `Import all (${waiting.length})`}
            </button>
          {/if}
        </div>
        <ul class="external">
          {#each external as instance (instance.gameDir)}
            {@const progress = imports[instance.gameDir]}
            <li>
              <div class="about">
                <span class="truncate">{instance.name}</span>
                <span class="faint small truncate">{LAUNCHERS[instance.launcher]}, {externalVersion(instance)}, {loaderName(instance.loader.kind)}</span>
                {#if progress?.status === 'failed'}<span class="small failed">{progress.reason}</span>{/if}
              </div>
              {#if progress?.status === 'importing'}
                <button class="btn" disabled>Importing</button>
              {:else if progress?.status === 'done'}
                <span class="imported">Imported</span>
                <button class="btn" onclick={() => openImported(progress.id)}>Open</button>
              {:else if progress?.status === 'failed'}
                <button class="btn" disabled={!!importingAll} onclick={() => importOne(instance)}>Try again</button>
              {:else if inLibrary(instance)}
                <span class="faint small">In your library</span>
                <button class="btn" disabled={!!importingAll} onclick={() => importOne(instance)}>Import again</button>
              {:else}
                <button class="btn" disabled={!!importingAll} onclick={() => importOne(instance)}>Import</button>
              {/if}
            </li>
          {/each}
        </ul>
      {:else if searched}
        <p class="muted intro">No instances found from the Minecraft Launcher, Prism Launcher, MultiMC, CurseForge, the Modrinth App, ATLauncher or GDLauncher.</p>
      {:else}
        <p class="muted intro">Looking for other launchers</p>
      {/if}
      <form class="paste" onsubmit={importCode}>
        <input class="field mono" placeholder="Paste a share code" aria-label="Share code" bind:value={code} />
        <button class="btn" disabled={!code.trim()}>Use code</button>
      </form>
      <div class="actions start">
        <button class="btn" disabled={!inTauri} onclick={importFile}>Install a .mrpack or .zip file</button>
        <span class="spacer"></span>
        <button class="btn" onclick={close}>Done</button>
      </div>
    {/if}
  </div>
</Modal>

<style>
  .picker { margin: -6px 0 6px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; padding-top: 12px; border-top: 1px solid var(--line); }
  .actions.start { justify-content: flex-start; border-top: 0; padding-top: 4px; }
  .spacer { flex: 1; }
  .paste { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; margin-bottom: 8px; }
  .intro { margin: 8px 0 10px; }
  .found { display: flex; align-items: center; gap: 12px; margin: 8px 0 10px; }
  .found p { flex: 1; min-width: 0; }
  .note { margin: -4px 0 14px; }
  .released { margin-top: 6px; }
  .check { display: flex; align-items: center; gap: 8px; padding-top: 8px; color: var(--text); }
  .small-check { color: var(--text-2); font-size: var(--text-xs); }
  .names { margin-top: 4px; }
  .external { max-height: 46vh; margin-bottom: 10px; overflow-y: auto; border-top: 1px solid var(--line); }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 0;
    border-bottom: 1px solid var(--line);
  }
  .about { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .failed { color: var(--bad); }
  .imported { color: var(--good); font-weight: 600; }
</style>

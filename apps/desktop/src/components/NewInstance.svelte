<script lang="ts">
  import { onMount } from 'svelte';
  import { api, inTauri, pickPackFile } from '../lib/api';
  import type { ExternalInstance, GameVersion, LoaderKind } from '../lib/api';
  import { day, loaderName } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

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
  const LAUNCHERS = { prism: 'Prism Launcher', multimc: 'MultiMC', curseforge: 'CurseForge', modrinth: 'Modrinth App', atlauncher: 'ATLauncher', gdlauncher: 'GDLauncher' };
  /** Fabric only reaches back to 1.14, and these projects carry it further. */
  const OLD_MINOR = 14;

  let mode = $state<'new' | 'import'>('new');
  let name = $state('');
  let minecraft = $state('');
  let loader = $state<LoaderKind>('fabric');
  let era = $state('release');
  let versions = $state<GameVersion[]>([]);
  let external = $state<ExternalInstance[]>([]);
  let code = $state('');
  let missing = $state<LoaderKind[]>([]);

  const shown = $derived(versions.filter((v) => v.era === era));
  const picked = $derived(versions.find((v) => v.id === minecraft));
  const candidates = $derived.by((): LoaderKind[] => {
    if (minecraft === 'b1.7.3') return ['vanilla', 'babric', 'ornithe'];
    if (picked && picked.era !== 'release' && picked.era !== 'snapshot') return ['vanilla'];
    const minor = /^1\.(\d+)/.exec(minecraft)?.[1];
    return minor && Number(minor) < OLD_MINOR ? OLD_LOADERS : LOADERS;
  });
  const loaders = $derived(candidates.filter((kind) => !missing.includes(kind)));

  $effect(() => {
    if (!loaders.includes(loader)) loader = 'vanilla';
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

  function close() {
    app.dialog = 'none';
  }

  onMount(async () => {
    const [manifest, found] = await Promise.allSettled([api.minecraftVersions(), api.externalInstances()]);
    if (manifest.status === 'fulfilled') {
      versions = manifest.value.versions;
      minecraft = manifest.value.latest;
    } else app.fail(manifest.reason, 'Could not load Minecraft versions');
    if (found.status === 'fulfilled') external = found.value;
  });

  async function create(event: SubmitEvent) {
    event.preventDefault();
    close();
    try {
      const instance = await api.createInstance({ name: name.trim(), minecraft, loader });
      await app.refresh();
      app.open(instance.id);
    } catch (error) {
      app.fail(error, 'Could not create the instance');
    }
  }

  async function importOne(instance: ExternalInstance) {
    close();
    try {
      const created = await api.importExternal(instance);
      await app.refresh();
      app.open(created.id);
    } catch (error) {
      app.fail(error, 'Could not import that instance');
    }
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
      const result = await api.installPackFile(path);
      await app.refresh();
      app.open(result.instance.id);
    } catch (error) {
      app.fail(error, 'Could not install that pack');
    }
  }
</script>

<Modal title="Add an instance" width={500} onclose={close}>
  <div class="tabs picker">
    <button class:active={mode === 'new'} onclick={() => (mode = 'new')}>New</button>
    <button class:active={mode === 'import'} onclick={() => (mode = 'import')}>Import{external.length ? ` (${external.length})` : ''}</button>
  </div>

  {#if mode === 'new'}
    <form onsubmit={create}>
      <div class="form-row">
        <label for="name">Name</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="name" class="field" bind:value={name} autofocus />
      </div>
      <div class="form-row">
        <label for="era">Era</label>
        <select id="era" class="field" value={era} onchange={(event) => chooseEra(event.currentTarget.value)}>
          {#each ERAS as item (item.id)}<option value={item.id}>{item.label}</option>{/each}
        </select>
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
      {#if loader === 'cleanroom'}
        <p class="muted note">Cleanroom warns on its first launch that two patch mods, Fugue and Scalar, are missing. Both can be installed from Discover.</p>
      {/if}
      <div class="actions">
        <button type="button" class="btn" onclick={close}>Cancel</button>
        <button class="btn primary" disabled={!name.trim() || !minecraft}>Create</button>
      </div>
    </form>
  {:else}
    {#if external.length}
      <p class="muted intro">Found on this computer. Importing makes a copy and leaves the original alone.</p>
      <ul>
        {#each external as instance (instance.gameDir)}
          <li>
            <div class="truncate">
              {instance.name}
              <span class="faint small">{LAUNCHERS[instance.launcher]}, {instance.minecraft}, {loaderName(instance.loader.kind)}</span>
            </div>
            <button class="btn" onclick={() => importOne(instance)}>Import</button>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="muted intro">No instances found from Prism Launcher, MultiMC, CurseForge, the Modrinth App, ATLauncher or GDLauncher.</p>
    {/if}
    <form class="paste" onsubmit={importCode}>
      <input class="field mono" placeholder="Paste a share code" aria-label="Share code" bind:value={code} />
      <button class="btn" disabled={!code.trim()}>Use code</button>
    </form>
    <div class="actions start">
      <button class="btn" disabled={!inTauri} onclick={importFile}>Install a .mrpack or .zip file</button>
    </div>
  {/if}
</Modal>

<style>
  .picker { margin: -6px 0 6px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; padding-top: 12px; border-top: 1px solid var(--line); }
  .actions.start { justify-content: flex-start; border-top: 0; padding-top: 4px; }
  .paste { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; margin-bottom: 8px; }
  .intro { margin: 8px 0 10px; }
  .note { margin: -4px 0 14px; }
  .released { margin-top: 6px; }
  ul { margin-bottom: 10px; border-top: 1px solid var(--line); }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 7px 0;
    border-bottom: 1px solid var(--line);
  }
  li span { margin-left: 6px; }
</style>

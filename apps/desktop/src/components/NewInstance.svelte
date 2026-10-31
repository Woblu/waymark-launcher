<script lang="ts">
  import { onMount } from 'svelte';
  import { api, inTauri, pickPackFile } from '../lib/api';
  import type { ExternalInstance, LoaderKind } from '../lib/api';
  import { loaderName } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  const LOADERS: LoaderKind[] = ['vanilla', 'fabric', 'neoforge', 'forge', 'quilt'];
  const LAUNCHERS = { prism: 'Prism Launcher', multimc: 'MultiMC', curseforge: 'CurseForge', modrinth: 'Modrinth App' };

  let mode = $state<'new' | 'import'>('new');
  let name = $state('');
  let minecraft = $state('');
  let loader = $state<LoaderKind>('fabric');
  let snapshots = $state(false);
  let versions = $state<{ id: string; kind: string }[]>([]);
  let external = $state<ExternalInstance[]>([]);

  const shown = $derived(versions.filter((v) => v.kind === 'release' || (snapshots && v.kind === 'snapshot')));

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
        <label for="version">Minecraft version</label>
        <div>
          <select id="version" class="field" bind:value={minecraft}>
            {#each shown as version (version.id)}<option value={version.id}>{version.id}</option>{/each}
          </select>
          <label class="check"><input type="checkbox" bind:checked={snapshots} /> Show snapshots</label>
        </div>
      </div>
      <div class="form-row">
        <label for="loader">Mod loader</label>
        <select id="loader" class="field" bind:value={loader}>
          {#each LOADERS as kind (kind)}<option value={kind}>{loaderName(kind)}</option>{/each}
        </select>
      </div>
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
      <p class="muted intro">No instances found from Prism Launcher, MultiMC, CurseForge or the Modrinth App.</p>
    {/if}
    <div class="actions start">
      <button class="btn" disabled={!inTauri} onclick={importFile}>Install a .mrpack or .zip file</button>
    </div>
  {/if}
</Modal>

<style>
  .picker { margin: -6px 0 6px; }
  .check { display: flex; align-items: center; gap: 7px; margin-top: 8px; color: var(--text-2); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; padding-top: 12px; border-top: 1px solid var(--line); }
  .actions.start { justify-content: flex-start; border-top: 0; padding-top: 4px; }
  .intro { margin: 8px 0 10px; }
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

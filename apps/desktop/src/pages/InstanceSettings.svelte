<script lang="ts">
  import { untrack } from 'svelte';
  import { api, openFolder, pickFolder } from '../lib/api';
  import type { LibraryEntry } from '../lib/api';
  import { app } from '../lib/state.svelte';

  let { entry }: { entry: LibraryEntry } = $props();

  const FOLDERS = [
    { id: 'resourcepacks', label: 'Resource packs' },
    { id: 'shaderpacks', label: 'Shader packs' },
    { id: 'screenshots', label: 'Screenshots' },
  ];

  const original = untrack(() => entry);
  let name = $state(original.name);
  let memory = $state(original.memoryMb ?? 4096);
  let useDefaultMemory = $state(original.memoryMb === undefined);
  let javaPath = $state(original.javaPath ?? '');
  let jvmArgs = $state((original.jvmArgs ?? []).join(' '));
  let confirming = $state(false);
  let copyFrom = $state('');
  let exporting = $state(false);

  const others = $derived(app.library.filter((other) => other.id !== entry.id));
  const shared = $derived(new Set(entry.shared ?? []));

  async function save(event: SubmitEvent) {
    event.preventDefault();
    try {
      await api.updateInstance(entry.id, {
        name: name.trim() || entry.name,
        memoryMb: useDefaultMemory ? null : memory,
        javaPath: javaPath.trim() || null,
        jvmArgs: jvmArgs.trim() ? jvmArgs.trim().split(/\s+/) : [],
      });
      await app.refresh();
      app.toast('good', 'Settings saved');
    } catch (error) {
      app.fail(error, 'Could not save');
    }
  }

  async function setShared(folder: string, on: boolean) {
    try {
      await api.setFolderShared(entry.id, folder, on);
      await app.refresh();
    } catch (error) {
      app.fail(error, 'Could not change that folder');
    }
  }

  async function copySettings() {
    const source = app.entry(copyFrom);
    try {
      await api.copySettings(copyFrom, entry.id);
      app.contentTick++;
      await app.refresh();
      app.toast('good', `Copied settings from ${source?.name ?? 'that instance'}`, 'You can undo this from Waymarks.');
      copyFrom = '';
    } catch (error) {
      app.fail(error, 'Could not copy settings');
    }
  }

  async function exportServer() {
    const folder = await pickFolder();
    if (!folder) return;
    exporting = true;
    try {
      const result = await api.exportServer(entry.id, folder);
      const left = result.skipped.length;
      app.toast('good', 'Server folder created', `${result.mods} mods copied${left ? `, ${left} client-only left out` : ''}. See README.txt in the folder.`);
      await openFolder(result.folder);
    } catch (error) {
      app.fail(error, 'Could not export the server');
    }
    exporting = false;
  }

  async function remove() {
    try {
      await api.deleteInstance(entry.id);
      await app.refresh();
      app.selected = null;
    } catch (error) {
      app.fail(error, 'Could not delete this instance');
    }
  }
</script>

<div class="tab">
  <form onsubmit={save}>
    <div class="form-row">
      <label for="instance-name">Name</label>
      <input id="instance-name" class="field" bind:value={name} />
    </div>
    <div class="form-row">
      <span class="form-label">Memory</span>
      <div>
        <label class="check"><input type="checkbox" bind:checked={useDefaultMemory} /> Use the default from Settings</label>
        {#if !useDefaultMemory}
          <div class="slider">
            <input type="range" min="1024" max="32768" step="512" aria-label="Memory" bind:value={memory} />
            <span class="tabular">{(memory / 1024).toFixed(1)} GB</span>
          </div>
        {/if}
      </div>
    </div>
    <div class="form-row">
      <label for="java">Java path</label>
      <div>
        <input id="java" class="field mono" bind:value={javaPath} />
        <p class="form-hint">Leave empty to use the version Mojang recommends for this release.</p>
      </div>
    </div>
    <div class="form-row">
      <label for="args">Java arguments</label>
      <input id="args" class="field mono" bind:value={jvmArgs} />
    </div>
    <div class="form-row">
      <span></span>
      <div><button class="btn primary">Save</button></div>
    </div>
  </form>

  <div class="form-row">
    <span class="form-label">Shared folders</span>
    <div>
      {#each FOLDERS as folder (folder.id)}
        <label class="check">
          <input type="checkbox" checked={shared.has(folder.id)} onchange={(event) => setShared(folder.id, event.currentTarget.checked)} />
          {folder.label}
        </label>
      {/each}
      <p class="form-hint">Instances that share a folder see the same files. Turning this off leaves this instance with its own copy.</p>
    </div>
  </div>

  {#if others.length}
    <div class="form-row">
      <label for="copy-from">Copy settings from</label>
      <div>
        <div class="line">
          <select id="copy-from" class="field" bind:value={copyFrom}>
            <option value="">Choose an instance</option>
            {#each others as other (other.id)}<option value={other.id}>{other.name}</option>{/each}
          </select>
          <button class="btn" disabled={!copyFrom} onclick={copySettings}>Copy</button>
        </div>
        <p class="form-hint">Copies game options and keybinds. This leaves a waymark, so it can be undone.</p>
      </div>
    </div>
  {/if}

  <div class="form-row">
    <span class="form-label">Server</span>
    <div>
      <button class="btn" disabled={exporting} onclick={exportServer}>{exporting ? 'Exporting' : 'Export as a server'}</button>
      <p class="form-hint">Builds a server folder with your mods and configs, and leaves out client-only mods. You accept the Minecraft EULA on its first run.</p>
    </div>
  </div>

  <div class="form-row">
    <span class="form-label">Delete instance</span>
    <div>
      {#if confirming}
        <p class="confirm">This removes its worlds and files from your computer and can't be undone.</p>
        <button class="btn" onclick={() => (confirming = false)}>Cancel</button>
        <button class="btn danger" onclick={remove}>Delete {entry.name}</button>
      {:else}
        <button class="btn danger" onclick={() => (confirming = true)}>Delete</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .tab { height: 100%; overflow-y: auto; padding: 4px 24px 16px; }
  form, .tab > .form-row { max-width: 640px; }
  .tab > .form-row:first-of-type { border-top: 1px solid var(--line); }
  .check { display: flex; align-items: center; gap: 7px; padding-top: 7px; }
  .line { display: flex; gap: 8px; }
  .slider { display: flex; align-items: center; gap: 12px; margin-top: 10px; }
  .slider input { flex: 1; }
  .slider span { width: 56px; text-align: right; }
  .confirm { margin-bottom: 8px; color: var(--text-2); }
</style>

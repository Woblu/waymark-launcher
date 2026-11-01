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
  let width = $state(original.width ? String(original.width) : '');
  let height = $state(original.height ? String(original.height) : '');
  let fullscreen = $state(original.fullscreen ?? false);
  let env = $state((original.env ?? []).map(([key, value]) => `${key}=${value}`).join('\n'));
  let preLaunch = $state(original.preLaunch ?? '');
  let wrapper = $state(original.wrapper ?? '');
  let postExit = $state(original.postExit ?? '');
  let confirming = $state(false);
  let copyFrom = $state('');
  let exporting = $state(false);

  const others = $derived(app.library.filter((other) => other.id !== entry.id));
  const shared = $derived(new Set(entry.shared ?? []));

  function parseEnv(text: string): [string, string][] {
    return text
      .split('\n')
      .map((line) => line.trim())
      .filter((line) => line.includes('='))
      .map((line) => [line.slice(0, line.indexOf('=')).trim(), line.slice(line.indexOf('=') + 1)]);
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    try {
      await api.updateInstance(entry.id, {
        name: name.trim() || entry.name,
        memoryMb: useDefaultMemory ? null : memory,
        javaPath: javaPath.trim() || null,
        jvmArgs: jvmArgs.trim() ? jvmArgs.trim().split(/\s+/) : [],
        options: {
          width: Number(width) || null,
          height: Number(height) || null,
          fullscreen,
          env: parseEnv(env),
          preLaunch: preLaunch.trim() || null,
          wrapper: wrapper.trim() || null,
          postExit: postExit.trim() || null,
        },
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
      const item = await api.deleteInstance(entry.id);
      await app.refresh();
      app.selected = null;
      app.toast('good', `Deleted ${entry.name}`, 'You can restore it from Settings for 30 days.', { label: 'Undo', run: () => app.restore(item.id) });
    } catch (error) {
      app.fail(error, 'Could not delete this instance');
    }
  }
</script>

<div class="tab">
  <div class="sheet">
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
      <span class="form-label">Game window</span>
      <div>
        <div class="size">
          <input class="field tabular" inputmode="numeric" placeholder="Width" aria-label="Window width" disabled={fullscreen} bind:value={width} />
          <span class="faint">by</span>
          <input class="field tabular" inputmode="numeric" placeholder="Height" aria-label="Window height" disabled={fullscreen} bind:value={height} />
        </div>
        <label class="check"><input type="checkbox" bind:checked={fullscreen} /> Start in fullscreen</label>
      </div>
    </div>
    <div class="form-row">
      <label for="env">Environment variables</label>
      <div>
        <textarea id="env" class="field mono area" rows="3" placeholder="NAME=value" bind:value={env}></textarea>
        <p class="form-hint">One on each line.</p>
      </div>
    </div>
    <div class="form-row">
      <label for="pre-launch">Before launching</label>
      <input id="pre-launch" class="field mono" bind:value={preLaunch} />
    </div>
    <div class="form-row">
      <label for="wrapper">Wrapper command</label>
      <div>
        <input id="wrapper" class="field mono" bind:value={wrapper} />
        <p class="form-hint">Runs the game through another program, like mangohud.</p>
      </div>
    </div>
    <div class="form-row">
      <label for="post-exit">After the game closes</label>
      <div>
        <input id="post-exit" class="field mono" bind:value={postExit} />
        <p class="form-hint">Commands run on your computer, so only use ones you trust. They can read INST_NAME, INST_DIR, INST_MC_DIR and INST_MC_VER.</p>
      </div>
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
        <p class="confirm">This removes it from your library, with its worlds. You can restore it from Settings for 30 days.</p>
        <button class="btn" onclick={() => (confirming = false)}>Cancel</button>
        <button class="btn danger" onclick={remove}>Delete {entry.name}</button>
      {:else}
        <button class="btn danger" onclick={() => (confirming = true)}>Delete</button>
      {/if}
    </div>
  </div>
  </div>
</div>

<style>
  .tab { height: 100%; overflow-y: auto; padding: 8px var(--gutter) var(--gutter); }
  .sheet { max-width: 720px; }
  .check { display: flex; align-items: center; gap: 8px; padding-top: 8px; }
  .size { display: flex; align-items: center; gap: 8px; }
  .size .field { width: 96px; }
  .area { height: auto; padding: 8px 12px; resize: vertical; }
  .line { display: flex; gap: 8px; }
  .slider { display: flex; align-items: center; gap: 12px; margin-top: 10px; }
  .slider input { flex: 1; }
  .slider span { width: 56px; text-align: right; }
  .confirm { margin-bottom: 8px; color: var(--text-2); }
</style>

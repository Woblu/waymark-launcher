<script lang="ts">
  import { onDestroy, tick, untrack } from 'svelte';
  import { api, openFolder, pickFolder } from '../lib/api';
  import type { LibraryEntry } from '../lib/api';
  import { number } from '../lib/format';
  import { app } from '../lib/state.svelte';

  let { entry }: { entry: LibraryEntry } = $props();

  const FOLDERS = [
    { id: 'resourcepacks', label: 'Resource packs' },
    { id: 'shaderpacks', label: 'Shader packs' },
    { id: 'screenshots', label: 'Screenshots' },
  ];

  const original = untrack(() => entry);
  /** What the form last matched on disk: when it opened, then after each save. */
  let base: LibraryEntry = original;
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
  let keepOwn = $state(original.keepsOwnSettings ?? false);
  /** Whether game settings are shared, which is when keeping its own means anything. */
  let sharing = $state(false);
  api.settings().then((found) => (sharing = found.shareSettings), () => {});
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

  async function unlink() {
    try {
      await api.updateInstance(entry.id, { server: null });
      await app.refresh();
      base = app.library.find((other) => other.id === entry.id) ?? base;
      app.toast('good', `${entry.name} no longer joins a server on Play`);
    } catch (error) {
      app.fail(error, 'Could not save');
    }
  }

  /**
   * What this form shows came from when it opened or last saved. Something outside Waymark can
   * change the instance meanwhile, like an installer adding a pre-launch command,
   * so a field this form didn't change keeps whatever is saved now.
   */
  function unlessChanged<T>(mine: T, opened: T, saved: T): T {
    return JSON.stringify(mine) === JSON.stringify(opened) ? saved : mine;
  }

  /** Everything the form holds, to tell whether there's anything left to save. */
  function values(): string {
    return JSON.stringify([name, useDefaultMemory ? null : memory, javaPath, jvmArgs, width, height, fullscreen, env, preLaunch, wrapper, postExit, keepOwn]);
  }

  let savedValues = $state(values());
  const dirty = $derived(values() !== savedValues);
  /** Set once the instance is deleted, so leaving doesn't try to save it. */
  let gone = false;

  async function save(event: SubmitEvent) {
    event.preventDefault();
    await persist();
  }

  // Leaving with changes saves them, as the other launchers do, rather than dropping them.
  onDestroy(() => {
    if (dirty && !gone) persist();
  });

  async function persist() {
    const id = original.id;
    const fallbackName = original.name;
    const snapshot = values();
    try {
      await app.refresh();
      const saved = app.library.find((other) => other.id === id) ?? original;
      const field = <T,>(mine: T, read: (from: LibraryEntry) => T) => unlessChanged(mine, read(base), read(saved));
      await api.updateInstance(id, {
        name: field(name.trim() || fallbackName, (from) => from.name),
        memoryMb: field(useDefaultMemory ? null : memory, (from) => from.memoryMb ?? null),
        javaPath: field(javaPath.trim() || null, (from) => from.javaPath ?? null),
        jvmArgs: field(jvmArgs.trim() ? jvmArgs.trim().split(/\s+/) : [], (from) => from.jvmArgs ?? []),
        keepsOwnSettings: field(keepOwn, (from) => from.keepsOwnSettings ?? false),
        options: {
          width: field(Number(width) || null, (from) => from.width ?? null),
          height: field(Number(height) || null, (from) => from.height ?? null),
          fullscreen: field(fullscreen, (from) => from.fullscreen ?? false),
          env: field(parseEnv(env), (from) => from.env ?? []),
          preLaunch: field(preLaunch.trim() || null, (from) => from.preLaunch ?? null),
          wrapper: field(wrapper.trim() || null, (from) => from.wrapper ?? null),
          postExit: field(postExit.trim() || null, (from) => from.postExit ?? null),
        },
      });
      await app.refresh();
      base = app.library.find((other) => other.id === id) ?? base;
      savedValues = snapshot;
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
      const mark = await api.copySettings(copyFrom, entry.id);
      app.contentTick++;
      await app.refresh();
      const title = `Copied settings from ${source?.name ?? 'that instance'}`;
      app.changed(entry.id, mark ?? title, undefined, title);
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
      // A sentence a line, so each one is translated on its own.
      const lines = [result.mods === 1 ? '1 mod copied.' : `${result.mods} mods copied.`];
      if (left) lines.push(left === 1 ? '1 client-only mod left out.' : `${left} client-only mods left out.`);
      lines.push('See README.txt in the folder.');
      app.toast('good', 'Server folder created', lines.join('\n'));
      await openFolder(result.folder);
    } catch (error) {
      app.fail(error, 'Could not export the server');
    }
    exporting = false;
  }

  async function remove() {
    gone = true;
    try {
      const item = await api.deleteInstance(entry.id);
      await app.refresh();
      app.selected = null;
      app.toast('good', `Deleted ${entry.name}`, 'You can restore it from Settings for 30 days.', { label: 'Undo', run: () => app.restore(item.id) });
    } catch (error) {
      gone = false;
      app.fail(error, 'Could not delete this instance');
    }
  }

  // Arriving from the crash dialog puts the cursor on the setting it was about,
  // whether settings open for it or were open already.
  $effect(() => {
    const wanted = app.focusField;
    if (!wanted) return;
    untrack(() => (app.focusField = null));
    tick().then(() => {
      const field = document.getElementById(wanted === 'memory' ? 'memory-default' : wanted);
      field?.scrollIntoView({ block: 'center' });
      field?.focus();
    });
  });
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
        <label class="check"><input id="memory-default" type="checkbox" bind:checked={useDefaultMemory} /> Use the default from Settings</label>
        {#if !useDefaultMemory}
          <div class="slider">
            <input type="range" min="1024" max="32768" step="512" aria-label="Memory" bind:value={memory} />
            <span class="tabular">{number(memory / 1024, 1)} GB</span>
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
    {#if entry.server}
      <div class="form-row">
        <span class="form-label">Server</span>
        <div>
          <p class="linked"><span class="name">{entry.server.name}</span> <span class="faint mono">{entry.server.address}</span></p>
          <p class="form-hint">Play joins this server straight away, and joining it from Servers keeps this instance on the version it needs.</p>
          <button type="button" class="btn" onclick={unlink}>Stop joining it on Play</button>
        </div>
      </div>
    {/if}
    {#if sharing}
      <div class="form-row">
        <span class="form-label">Game settings</span>
        <div>
          <label class="check"><input type="checkbox" bind:checked={keepOwn} /> Keep this instance's own game settings</label>
          <p class="form-hint">Its keybinds, video and other options stay as they are, and what you change here stays here.</p>
        </div>
      </div>
    {/if}
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
      <div class="save"><button class="btn primary">Save</button>{#if dirty}<span class="faint small">Unsaved changes. Leaving this page saves them.</span>{/if}</div>
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
  .linked { padding-top: 8px; }
  .linked .name { font-weight: 600; }
  .linked + .form-hint + .btn { margin-top: 8px; }
  .size { display: flex; align-items: center; gap: 8px; }
  .size .field { width: 96px; }
  .area { height: auto; padding: 8px 12px; resize: vertical; }
  .line { display: flex; gap: 8px; }
  .slider { display: flex; align-items: center; gap: 12px; margin-top: 10px; }
  .slider input { flex: 1; }
  .slider span { width: 56px; text-align: right; }
  .confirm { margin-bottom: 8px; color: var(--text-2); }
  .save { display: flex; align-items: center; gap: 12px; }
</style>

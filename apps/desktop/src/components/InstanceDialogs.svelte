<script lang="ts">
  import { api, pickSavePath } from '../lib/api';
  import type { LibraryEntry, ShareCode } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { InstanceDialog } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  let { dialog }: { dialog: InstanceDialog } = $props();

  type Format = 'modrinth' | 'curseforge' | 'list';
  const LIST_FORMATS = [
    { id: 'text', label: 'Plain text' },
    { id: 'markdown', label: 'Markdown' },
    { id: 'csv', label: 'CSV' },
  ] as const;

  const entry = $derived(app.entry(dialog.id) as LibraryEntry | undefined);
  const groups = $derived([...new Set(app.library.flatMap((other) => (other.group ? [other.group] : [])))].sort());

  let name = $state('');
  let worlds = $state(false);
  let group = $state('');
  let newGroup = $state('');
  let format = $state<Format>('modrinth');
  let listFormat = $state<'text' | 'markdown' | 'csv'>('markdown');
  let version = $state('1.0.0');
  let busy = $state(false);
  let share = $state<ShareCode | null>(null);

  $effect(() => {
    if (dialog.kind === 'share') api.shareCode(dialog.id).then((found) => (share = found), (error) => app.fail(error));
  });

  async function copyCode() {
    if (!share) return;
    await navigator.clipboard.writeText(share.code);
    app.toast('good', 'Code copied');
  }

  $effect(() => {
    if (!entry) return;
    name = dialog.kind === 'copy' ? `${entry.name} copy` : entry.name;
    group = entry.group ?? '';
  });

  const close = () => (app.instanceDialog = null);

  async function copy(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    try {
      const made = await api.copyInstance(dialog.id, name.trim(), worlds);
      await app.refresh();
      close();
      app.open(made.id);
    } catch (error) {
      app.fail(error, 'Could not copy this instance');
    }
    busy = false;
  }

  async function saveGroup(event: SubmitEvent) {
    event.preventDefault();
    const chosen = group === '+new' ? newGroup.trim() : group;
    try {
      await api.setInstanceGroup(dialog.id, chosen || null);
      await app.refresh();
      close();
    } catch (error) {
      app.fail(error, 'Could not move this instance');
    }
  }

  async function exportPack(event: SubmitEvent) {
    event.preventDefault();
    if (format === 'list') {
      const text = await api.modList(dialog.id, listFormat);
      await navigator.clipboard.writeText(text);
      app.toast('good', 'Copied the list');
      close();
      return;
    }
    const extension = format === 'modrinth' ? 'mrpack' : 'zip';
    const target = await pickSavePath(`${name.trim() || 'pack'}-${version.trim()}.${extension}`, format === 'modrinth' ? 'Modrinth pack' : 'CurseForge pack', extension);
    if (!target) return;
    busy = true;
    try {
      const report = await api.exportPack(dialog.id, target, format, name.trim(), version.trim());
      const inside = report.bundled.length;
      const note = inside ? ` ${inside} ${inside === 1 ? 'file is' : 'files are'} inside the pack, so check you're allowed to share ${inside === 1 ? 'it' : 'them'}: ${report.bundled.slice(0, 6).join(', ')}${inside > 6 ? ' and more' : ''}.` : '';
      app.toast('good', 'Pack exported', `${report.linked} files linked to the store.${note}`);
      close();
    } catch (error) {
      app.fail(error, 'Could not export this instance');
    }
    busy = false;
  }
</script>

{#if entry && dialog.kind === 'copy'}
  <Modal title="Copy instance" width={440} onclose={close}>
    <form onsubmit={copy}>
      <div class="form-row">
        <label for="copy-name">Name</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="copy-name" class="field" bind:value={name} autofocus />
      </div>
      <div class="form-row">
        <span class="form-label">Worlds</span>
        <div>
          <label class="check"><input type="checkbox" bind:checked={worlds} /> Copy worlds too</label>
          <p class="form-hint">Mods and packs are shared with the original on disk, so a copy takes up almost no extra space.</p>
        </div>
      </div>
      <div class="buttons">
        <button type="button" class="btn" onclick={close}>Cancel</button>
        <button class="btn primary" disabled={busy || !name.trim()}>{busy ? 'Copying' : 'Copy'}</button>
      </div>
    </form>
  </Modal>
{:else if entry && dialog.kind === 'group'}
  <Modal title="Move to a folder" width={400} onclose={close}>
    <form onsubmit={saveGroup}>
      <div class="form-row">
        <label for="group">Folder</label>
        <select id="group" class="field" bind:value={group}>
          <option value="">No folder</option>
          {#each groups as item (item)}<option value={item}>{item}</option>{/each}
          <option value="+new">New folder</option>
        </select>
      </div>
      {#if group === '+new'}
        <div class="form-row">
          <label for="new-group">Name</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input id="new-group" class="field" bind:value={newGroup} autofocus />
        </div>
      {/if}
      <div class="buttons">
        <button type="button" class="btn" onclick={close}>Cancel</button>
        <button class="btn primary" disabled={group === '+new' && !newGroup.trim()}>Move</button>
      </div>
    </form>
  </Modal>
{:else if entry && dialog.kind === 'share'}
  <Modal title="Share {entry.name}" width={520} onclose={close}>
    <p class="muted intro">Send this code to a friend. They paste it under Import when adding an instance, and Waymark downloads the same versions of everything from the stores.</p>
    {#if share}
      <textarea class="field mono code" readonly rows="5" aria-label="Share code" value={share.code}></textarea>
      {#if share.leftOut.length}
        <p class="form-hint">Left out because no store has them: {share.leftOut.slice(0, 6).join(', ')}{share.leftOut.length > 6 ? ' and more' : ''}.</p>
      {/if}
    {/if}
    <div class="buttons">
      <button class="btn" onclick={close}>Close</button>
      <button class="btn primary" disabled={!share} onclick={copyCode}>Copy code</button>
    </div>
  </Modal>
{:else if entry && dialog.kind === 'export'}
  <Modal title="Export {entry.name}" width={480} onclose={close}>
    <form onsubmit={exportPack}>
      <div class="form-row">
        <label for="format">Export as</label>
        <div>
          <select id="format" class="field" bind:value={format}>
            <option value="modrinth">Modrinth pack (.mrpack)</option>
            <option value="curseforge">CurseForge pack (.zip)</option>
            <option value="list">List of mods</option>
          </select>
          {#if format !== 'list'}
            <p class="form-hint">Whatever the store has is linked. Anything else goes inside the pack, and you'll be told which files those are.</p>
          {/if}
        </div>
      </div>
      {#if format === 'list'}
        <div class="form-row">
          <label for="list-format">Format</label>
          <select id="list-format" class="field" bind:value={listFormat}>
            {#each LIST_FORMATS as item (item.id)}<option value={item.id}>{item.label}</option>{/each}
          </select>
        </div>
      {:else}
        <div class="form-row">
          <label for="pack-name">Name</label>
          <input id="pack-name" class="field" bind:value={name} />
        </div>
        <div class="form-row">
          <label for="pack-version">Version</label>
          <input id="pack-version" class="field" bind:value={version} />
        </div>
      {/if}
      <div class="buttons">
        <button type="button" class="btn" onclick={close}>Cancel</button>
        <button class="btn primary" disabled={busy || (format !== 'list' && (!name.trim() || !version.trim()))}>
          {format === 'list' ? 'Copy list' : busy ? 'Exporting' : 'Export'}
        </button>
      </div>
    </form>
  </Modal>
{/if}

<style>
  .check { display: flex; align-items: center; gap: 7px; padding-top: 6px; cursor: pointer; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; padding-top: 12px; border-top: 1px solid var(--line); }
  .intro { margin-bottom: 10px; }
  .code { height: auto; padding: 6px 9px; resize: none; word-break: break-all; }
  .code + .buttons, .form-hint + .buttons { margin-top: 12px; }
</style>

<script lang="ts">
  import { untrack } from 'svelte';
  import { ChevronDown, ChevronRight } from '@lucide/svelte';
  import { api } from '../lib/api';
  import type { LibraryEntry, ModUpdate, VersionRow } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';
  import RichText from './RichText.svelte';

  let { entry, updates, onclose }: { entry: LibraryEntry; updates: ModUpdate[]; onclose: () => void } = $props();

  let chosen = $state(untrack(() => new Set(updates.filter((update) => !update.held).map((update) => update.hash))));
  let versions = $state<Record<string, VersionRow[]>>({});
  let picked = $state<Record<string, string>>({});
  let changelogs = $state<Record<string, string>>({});
  let open = $state<string | null>(null);
  let updating = $state(false);

  const fits = (row: VersionRow) => {
    if (!row.gameVersions.includes(entry.minecraft)) return false;
    const accepted = entry.loader.kind === 'quilt' ? ['quilt', 'fabric'] : [entry.loader.kind];
    return row.loaders.length === 0 || row.loaders.some((name) => accepted.includes(name)) || !row.loaders.some((name) => ['fabric', 'forge', 'neoforge', 'quilt'].includes(name));
  };

  async function showChangelog(update: ModUpdate, version: string) {
    if (version in changelogs) return;
    changelogs[version] = await api.versionChangelog(update.source, update.project, version).catch(() => '');
  }

  async function toggle(update: ModUpdate) {
    open = open === update.hash ? null : update.hash;
    if (!open) return;
    if (!versions[update.hash]) {
      const list = await api.projectVersions(update.source, update.project, { minecraft: entry.minecraft, offset: 0 }).catch(() => null);
      versions[update.hash] = list?.versions.filter(fits) ?? [];
    }
    const first = picked[update.hash] ?? versions[update.hash][0]?.id;
    if (first) await showChangelog(update, first);
  }

  function setChosen(update: ModUpdate, on: boolean) {
    const next = new Set(chosen);
    if (on) next.add(update.hash);
    else next.delete(update.hash);
    chosen = next;
  }

  async function pickVersion(update: ModUpdate, version: string) {
    picked[update.hash] = version;
    await showChangelog(update, version);
  }

  async function run() {
    updating = true;
    try {
      const picks = [...chosen].map((hash) => ({ hash, version: picked[hash] }));
      const mark = await api.applyUpdates(entry.id, picks);
      if (mark) app.toast('good', mark.label, 'You can undo this from Waymarks.');
      app.contentTick++;
      onclose();
    } catch (error) {
      app.fail(error, 'Could not update those');
    }
    updating = false;
  }
</script>

<Modal title="Update {entry.name}" width={640} {onclose}>
  <ul>
    {#each updates as update (update.hash)}
      {@const version = picked[update.hash] ?? versions[update.hash]?.[0]?.id}
      <li>
        <div class="row">
          <input type="checkbox" checked={chosen.has(update.hash)} aria-label="Update {update.name}" onchange={(event) => setChosen(update, event.currentTarget.checked)} />
          <button class="fold" aria-expanded={open === update.hash} onclick={() => toggle(update)}>
            {#if open === update.hash}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
            <span class="name truncate">{update.name}</span>
          </button>
          <span class="versions truncate muted">{update.current ?? 'Unknown'} to {update.latest}</span>
          {#if update.held}<span class="faint small" title="Too new to install under your Hold back setting">Held back</span>{/if}
        </div>
        {#if open === update.hash}
          <div class="detail">
            {#if versions[update.hash]?.length}
              <select class="field" aria-label="Version of {update.name}" value={version} onchange={(event) => pickVersion(update, event.currentTarget.value)}>
                {#each versions[update.hash] as row (row.id)}<option value={row.id}>{row.name}</option>{/each}
              </select>
            {/if}
            {#if version && version in changelogs}
              {#if changelogs[version].trim()}
                <div class="log"><RichText source={changelogs[version]} format={update.source === 'modrinth' ? 'markdown' : 'html'} base="https://modrinth.com" /></div>
              {:else}
                <p class="muted">No changelog for this version.</p>
              {/if}
            {:else}
              <p class="muted">Loading the changelog</p>
            {/if}
          </div>
        {/if}
      </li>
    {/each}
  </ul>
  <div class="buttons">
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" disabled={updating || !chosen.size} onclick={run}>{updating ? 'Updating' : `Update ${chosen.size}`}</button>
  </div>
</Modal>

<style>
  ul { max-height: 60vh; overflow-y: auto; border-top: 1px solid var(--line); }
  li { border-bottom: 1px solid var(--line); }
  .row { display: grid; grid-template-columns: 22px minmax(0, 1fr) minmax(0, 1fr) auto; gap: 8px; align-items: center; min-height: 40px; }
  .fold { display: flex; align-items: center; gap: 4px; min-width: 0; text-align: left; }
  .name { font-weight: 600; }
  .detail { display: flex; flex-direction: column; gap: 10px; padding: 0 0 12px 30px; }
  .log { max-height: 240px; overflow-y: auto; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; padding-top: 12px; }
</style>

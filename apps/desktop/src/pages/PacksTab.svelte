<script lang="ts">
  import { Compass, Download, Search, Trash2 } from '@lucide/svelte';
  import VirtualList from '../components/VirtualList.svelte';
  import { api, iconUrl } from '../lib/api';
  import type { ContentKind, LockedMod, ModUpdate } from '../lib/api';
  import { bytes } from '../lib/format';
  import { app } from '../lib/state.svelte';

  let { id, kind, shared }: { id: string; kind: ContentKind; shared: boolean } = $props();

  const ROW = 60;

  /** Whole sentences for each kind, so each one can be translated as it stands. */
  interface Words {
    search: (count: number) => string;
    searchLabel: string;
    update: (count: number) => string;
    browse: string;
    noMatch: string;
    none: string;
    failed: string;
  }
  const WORDS: Partial<Record<ContentKind, Words>> = {
    resourcepack: {
      search: (count) => (count === 1 ? 'Search 1 resource pack' : `Search ${count} resource packs`),
      searchLabel: 'Search resource packs',
      update: (count) => (count === 1 ? 'Update 1 resource pack' : `Update ${count} resource packs`),
      browse: 'Browse resource packs',
      noMatch: 'No resource packs match.',
      none: 'This instance has no resource packs.',
      failed: 'Could not update those resource packs',
    },
    shader: {
      search: (count) => (count === 1 ? 'Search 1 shader' : `Search ${count} shaders`),
      searchLabel: 'Search shaders',
      update: (count) => (count === 1 ? 'Update 1 shader' : `Update ${count} shaders`),
      browse: 'Browse shaders',
      noMatch: 'No shaders match.',
      none: 'This instance has no shaders.',
      failed: 'Could not update those shaders',
    },
  };

  let packs = $state<LockedMod[]>([]);
  let updates = $state<ModUpdate[]>([]);
  let query = $state('');
  let loaded = $state(false);
  let updating = $state(false);

  const words = $derived(WORDS[kind] ?? WORDS.resourcepack!);
  const ready = $derived(updates.filter((u) => !u.held));
  const updatable = $derived(new Set(ready.map((u) => u.hash)));
  const shown = $derived.by(() => {
    const text = query.trim().toLowerCase();
    return text ? packs.filter((pack) => pack.name.toLowerCase().includes(text)) : packs;
  });

  async function load() {
    try {
      packs = await api.content(id, kind);
    } catch (error) {
      app.fail(error, 'Could not read this instance');
    }
    loaded = true;
    const identified = await api.identifyContent(id, kind).catch(() => null);
    if (identified) packs = identified;
    const found = await api.checkUpdates(id).catch(() => []);
    updates = found.filter((update) => update.kind === kind);
  }

  $effect(() => {
    app.contentTick;
    load();
  });

  async function toggle(pack: LockedMod, enabled: boolean) {
    try {
      packs = await api.setContentEnabled(id, kind, pack.hash, enabled);
    } catch (error) {
      app.fail(error);
    }
  }

  async function remove(pack: LockedMod) {
    try {
      const mark = await api.removeContent(id, kind, [pack.hash]);
      if (mark) app.changed(id, mark);
      app.contentTick++;
    } catch (error) {
      app.fail(error);
    }
  }

  async function updateAll() {
    updating = true;
    try {
      const mark = await api.applyUpdates(id, ready.map((u) => ({ hash: u.hash })));
      if (mark) app.changed(id, mark);
      app.contentTick++;
      if (mark) app.checkUpdate(id);
    } catch (error) {
      app.fail(error, words.failed);
    }
    updating = false;
  }
</script>

<div class="tab">
  <div class="content-tools">
    <label class="search-box"><Search size={16} /><input class="field" placeholder={words.search(packs.length)} aria-label={words.searchLabel} bind:value={query} /></label>
    {#if shared}<span class="faint small">Shared with other instances</span>{/if}
    <span class="spacer"></span>
    {#if ready.length}
      <button class="btn primary" disabled={updating} onclick={updateAll}><Download size={16} /> {words.update(ready.length)}</button>
    {/if}
    <button class="btn" onclick={() => app.go('discover', id, kind)}><Compass size={16} /> {words.browse}</button>
  </div>

  {#if shown.length}
    <div class="content-list">
      <div class="content-head no-pick">
        <span class="content-cells"><span>Name</span><span>Version</span><span class="right">Size</span></span>
      </div>
      <div class="content-rows">
      <VirtualList items={shown} rowHeight={() => ROW}>
        {#snippet row(pack: LockedMod)}
          {@const image = iconUrl(pack.icon)}
          <div class="content-row no-pick pack" class:off={!pack.enabled}>
            <button class="content-cells body" onclick={() => app.openMod(id, pack)}>
              <span class="content-icon">{#if image}<img src={image} alt="" loading="lazy" />{:else}{pack.name.charAt(0).toUpperCase()}{/if}</span>
              <span class="name truncate">{pack.name}</span>
              <span class="version truncate">
                {pack.version ?? ''}{#if updatable.has(pack.hash)}<span class="update">Update available</span>{/if}
              </span>
              <span class="size tabular">{pack.size ? bytes(pack.size) : 'Folder'}</span>
            </button>
            <input type="checkbox" class="switch" checked={pack.enabled} aria-label="{pack.name} enabled" title={pack.enabled ? 'Turn off' : 'Turn on'} onchange={(event) => toggle(pack, event.currentTarget.checked)} />
            <button class="content-remove" aria-label="Remove {pack.name}" title="Remove" onclick={() => remove(pack)}><Trash2 size={15} /></button>
          </div>
        {/snippet}
      </VirtualList>
      </div>
    </div>
  {:else if loaded}
    <div class="blank">
      <p class="muted">{packs.length ? words.noMatch : words.none}</p>
    </div>
  {/if}
</div>

<style>
  .tab { display: flex; flex-direction: column; height: 100%; }
  .spacer { flex: 1; }
  .name { font-size: var(--text-m); font-weight: 600; }
  .body:hover .name { text-decoration: underline; text-underline-offset: 2px; }
  .version { color: var(--text-2); }
  .size { color: var(--text-3); }
  .off .body { opacity: 0.5; }
  .update { display: block; color: var(--accent-text); font-size: var(--text-xs); }
</style>

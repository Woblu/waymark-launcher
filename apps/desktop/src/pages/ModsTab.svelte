<script lang="ts">
  import { ChevronDown, ChevronRight, X } from '@lucide/svelte';
  import VirtualList from '../components/VirtualList.svelte';
  import { api, iconUrl } from '../lib/api';
  import type { LockedMod, ModUpdate } from '../lib/api';
  import { bytes } from '../lib/format';
  import { app } from '../lib/state.svelte';

  let { id }: { id: string } = $props();

  type Filter = 'all' | 'manual' | 'pack' | 'dependency' | 'disabled';
  type Row = { kind: 'group'; name: string; mods: LockedMod[] } | { kind: 'mod'; mod: LockedMod };

  const UNGROUPED = 'Ungrouped';
  const MOD_ROW = 58;
  const GROUP_ROW = 34;

  let mods = $state<LockedMod[]>([]);
  let updates = $state<ModUpdate[]>([]);
  let query = $state('');
  let filter = $state<Filter>('all');
  let loaded = $state(false);
  let updating = $state(false);
  let picked = $state(new Set<string>());
  let collapsed = $state(new Set<string>());
  let anchor = $state<string | null>(null);
  let naming = $state(false);
  let newGroup = $state('');
  let newTag = $state('');

  const ready = $derived(updates.filter((u) => !u.held));
  const held = $derived(updates.length - ready.length);
  const updatable = $derived(new Set(ready.map((u) => u.hash)));
  const groups = $derived([...new Set(mods.flatMap((m) => (m.group ? [m.group] : [])))].sort((a, b) => a.localeCompare(b)));

  const shown = $derived.by(() => {
    const text = query.trim().toLowerCase();
    return mods.filter((mod) => {
      const haystack = [mod.name, mod.group ?? '', ...(mod.tags ?? [])].join(' ').toLowerCase();
      if (text && !haystack.includes(text)) return false;
      if (filter === 'disabled') return !mod.enabled;
      return filter === 'all' || mod.origin.kind === filter;
    });
  });

  const rows = $derived.by((): Row[] => {
    if (!groups.length) return shown.map((mod) => ({ kind: 'mod', mod }));
    const out: Row[] = [];
    for (const name of [...groups, UNGROUPED]) {
      const members = shown.filter((m) => (m.group ?? UNGROUPED) === name);
      if (!members.length) continue;
      out.push({ kind: 'group', name, mods: members });
      if (!collapsed.has(name)) out.push(...members.map((mod): Row => ({ kind: 'mod', mod })));
    }
    return out;
  });

  const order = $derived(rows.flatMap((row) => (row.kind === 'mod' ? [row.mod.hash] : [])));
  const chosen = $derived([...picked]);

  async function load() {
    try {
      mods = await api.mods(id);
      updates = await api.checkUpdates(id);
    } catch (error) {
      if (!mods.length) app.fail(error, 'Could not read this instance');
    }
    loaded = true;
    const identified = await api.identifyMods(id).catch(() => null);
    if (identified) mods = identified;
  }

  $effect(() => {
    app.contentTick;
    load();
  });

  function clearPicked() {
    picked = new Set();
    anchor = null;
    naming = false;
  }

  /** Click picks one, Ctrl adds to the selection, and Shift picks a range. */
  function pick(event: MouseEvent, mod: LockedMod) {
    const next = new Set(event.ctrlKey || event.metaKey ? picked : []);
    if (event.shiftKey && anchor) {
      const [from, to] = [order.indexOf(anchor), order.indexOf(mod.hash)].sort((a, b) => a - b);
      for (const hash of order.slice(from, to + 1)) next.add(hash);
    } else if (next.has(mod.hash)) {
      next.delete(mod.hash);
    } else {
      next.add(mod.hash);
      anchor = mod.hash;
    }
    picked = next;
  }

  async function run(work: () => Promise<LockedMod[]>) {
    try {
      mods = await work();
    } catch (error) {
      app.fail(error);
    }
  }

  const toggle = (mod: LockedMod, enabled: boolean) => run(() => api.setModEnabled(id, mod.hash, enabled));
  const setEnabled = (hashes: string[], enabled: boolean) => run(() => api.setModsEnabled(id, hashes, enabled));

  async function setGroup(group: string | null) {
    await run(() => api.setModsGroup(id, chosen, group));
    naming = false;
    newGroup = '';
  }

  function onGroupPicked(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    const value = select.value;
    select.value = '';
    if (value === '+new') naming = true;
    else if (value === '-none') setGroup(null);
    else if (value) setGroup(value);
  }

  async function addTag(event: SubmitEvent) {
    event.preventDefault();
    const tag = newTag.trim();
    if (!tag) return;
    await run(() => api.toggleModsTag(id, chosen, tag));
    newTag = '';
  }

  function foldGroup(name: string) {
    const next = new Set(collapsed);
    if (!next.delete(name)) next.add(name);
    collapsed = next;
  }

  async function remove(hashes: string[]) {
    try {
      const mark = await api.removeMods(id, hashes);
      if (mark) app.toast('good', mark.label, 'You can undo this from Waymarks.');
      clearPicked();
      app.contentTick++;
      await app.refresh();
    } catch (error) {
      app.fail(error);
    }
  }

  async function updateAll() {
    updating = true;
    try {
      const mark = await api.applyUpdates(id, ready.map((u) => u.hash));
      if (mark) app.toast('good', mark.label, 'You can undo this from Waymarks.');
      app.contentTick++;
    } catch (error) {
      app.fail(error, 'Could not update those mods');
    }
    updating = false;
  }

  async function findCrash() {
    try {
      await api.startBisect(id);
      app.contentTick++;
      await app.refresh();
    } catch (error) {
      app.fail(error, 'Could not start the search');
    }
  }

  function origin(mod: LockedMod): string {
    if (mod.origin.kind === 'pack') return `From ${mod.origin.name}`;
    if (mod.origin.kind === 'dependency') return `Needed by ${mod.origin.of[0] ?? 'another mod'}`;
    return 'Added by you';
  }

  const rowHeight = (row: Row) => (row.kind === 'group' ? GROUP_ROW : MOD_ROW);
</script>

<div class="tab">
  {#if picked.size}
    <div class="tools selection" aria-label="Selected mods">
      <span>{picked.size} selected</span>
      {#if naming}
        <form class="inline" onsubmit={(event) => { event.preventDefault(); setGroup(newGroup); }}>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="field short" placeholder="Group name" aria-label="Group name" bind:value={newGroup} autofocus />
          <button class="btn" disabled={!newGroup.trim()}>Create</button>
          <button type="button" class="btn" onclick={() => (naming = false)}>Cancel</button>
        </form>
      {:else}
        <select class="field group-select" aria-label="Move to group" onchange={onGroupPicked}>
          <option value="">Move to group</option>
          {#each groups as group (group)}<option value={group}>{group}</option>{/each}
          <option value="+new">New group</option>
          <option value="-none">No group</option>
        </select>
        <form class="inline" onsubmit={addTag}>
          <input class="field short" placeholder="Add or remove a tag" aria-label="Tag" bind:value={newTag} />
        </form>
      {/if}
      <span class="spacer"></span>
      <button class="btn" onclick={() => setEnabled(chosen, true)}>Turn on</button>
      <button class="btn" onclick={() => setEnabled(chosen, false)}>Turn off</button>
      <button class="btn danger" onclick={() => remove(chosen)}>Remove</button>
      <button class="btn" onclick={clearPicked}>Clear</button>
    </div>
  {:else}
    <div class="tools">
      <input class="field search" placeholder="Search {mods.length} {mods.length === 1 ? 'mod' : 'mods'}" aria-label="Search mods" bind:value={query} />
      <select class="field filter" aria-label="Show" bind:value={filter}>
        <option value="all">All mods</option>
        <option value="manual">Added by you</option>
        <option value="pack">From the pack</option>
        <option value="dependency">Dependencies</option>
        <option value="disabled">Switched off</option>
      </select>
      <span class="spacer"></span>
      {#if held}<span class="faint small">{held} held back as too new</span>{/if}
      {#if ready.length}
        <button class="btn" disabled={updating} onclick={updateAll}>Update {ready.length} {ready.length === 1 ? 'mod' : 'mods'}</button>
      {/if}
      {#if mods.length > 1}
        <button class="btn" title="Tests your mods in groups to find the one causing a crash" onclick={findCrash}>Find a crash</button>
      {/if}
    </div>
  {/if}

  {#if rows.length}
    <div class="list">
      <VirtualList items={rows} {rowHeight}>
        {#snippet row(item: Row)}
          {#if item.kind === 'group'}
            {@const on = item.mods.filter((m) => m.enabled).length}
            <div class="group">
              <input
                type="checkbox"
                checked={on === item.mods.length}
                indeterminate={on > 0 && on < item.mods.length}
                aria-label="All mods in {item.name} on"
                onchange={(event) => setEnabled(item.mods.map((m) => m.hash), event.currentTarget.checked)}
              />
              <button class="fold" aria-expanded={!collapsed.has(item.name)} onclick={() => foldGroup(item.name)}>
                {#if collapsed.has(item.name)}<ChevronRight size={14} />{:else}<ChevronDown size={14} />{/if}
                {item.name}
              </button>
              <span class="faint small">{on} of {item.mods.length} on</span>
            </div>
          {:else}
            {@const mod = item.mod}
            {@const image = iconUrl(mod.icon)}
            <div class="mod" class:off={!mod.enabled} class:picked={picked.has(mod.hash)}>
              <input type="checkbox" checked={mod.enabled} aria-label="{mod.name} enabled" onchange={(event) => toggle(mod, event.currentTarget.checked)} />
              <button class="body" aria-pressed={picked.has(mod.hash)} onclick={(event) => pick(event, mod)}>
                <span class="icon">{#if image}<img src={image} alt="" loading="lazy" />{:else}{mod.name.charAt(0).toUpperCase()}{/if}</span>
                <span class="text">
                  <span class="name truncate">{mod.name}</span>
                  <span class="sub truncate">
                    {origin(mod)}{#each mod.tags ?? [] as tag (tag)}<span class="tag">{tag}</span>{/each}
                  </span>
                </span>
                <span class="version truncate">
                  {mod.version ?? ''}{#if updatable.has(mod.hash)}<span class="update">Update available</span>{/if}
                </span>
                <span class="size tabular">{bytes(mod.size)}</span>
              </button>
              <button class="remove" aria-label="Remove {mod.name}" title="Remove" onclick={() => remove([mod.hash])}><X size={15} /></button>
            </div>
          {/if}
        {/snippet}
      </VirtualList>
    </div>
  {:else if loaded}
    <div class="none">
      {#if mods.length}
        <p class="muted">No mods match.</p>
      {:else}
        <p class="muted">This instance has no mods.</p>
        <button class="btn" onclick={() => app.go('discover', id)}>Browse mods</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tab { display: flex; flex-direction: column; height: 100%; }
  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 50px;
    padding: 0 24px;
    border-bottom: 1px solid var(--line);
  }
  .selection { background: var(--raised); }
  .inline { display: flex; gap: 8px; }
  .search { width: 240px; }
  .filter { width: 150px; }
  .short { width: 170px; }
  .group-select { width: 150px; }
  .spacer { flex: 1; }
  .list { flex: 1; min-height: 0; }

  .group {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 18px 0 24px;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
  }
  .fold { display: flex; align-items: center; gap: 4px; font-weight: 600; }

  .mod {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) 26px;
    gap: 10px;
    align-items: center;
    height: 58px;
    padding: 0 18px 0 24px;
    border-bottom: 1px solid var(--line);
  }
  .mod:hover { background: var(--panel); }
  .mod.picked { background: var(--selected); }
  .body {
    display: grid;
    grid-template-columns: 42px minmax(0, 1.6fr) minmax(0, 1fr) 74px;
    gap: 12px;
    align-items: center;
    height: 100%;
    text-align: left;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    overflow: hidden;
    background: var(--control);
    color: var(--text-3);
    font-size: 17px;
    font-weight: 600;
  }
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .text { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .name { font-size: var(--text-m); font-weight: 600; }
  .sub { color: var(--text-3); font-size: var(--text-xs); }
  .tag { margin-left: 8px; color: var(--info); }
  .version { color: var(--text-2); }
  .size { color: var(--text-3); text-align: right; }
  .off .body { opacity: 0.45; }
  .update { display: block; color: var(--accent); font-size: var(--text-xs); }
  .remove { display: grid; place-items: center; width: 24px; height: 24px; color: var(--text-3); opacity: 0; }
  .mod:hover .remove, .remove:focus-visible { opacity: 1; }
  .remove:hover { color: var(--bad); }
  .none { padding: 20px 24px; }
  .none .btn { margin-top: 10px; }
</style>

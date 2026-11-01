<script lang="ts">
  import { ChevronDown, ChevronRight, Compass, Pencil, Play, Plus, Search, Trash2, X } from '@lucide/svelte';
  import { api, iconUrl, localImage, openFolder } from '../lib/api';
  import type { LockedMod, SavedServer, ServerStatus, World } from '../lib/api';
  import { bytes, timeAgo } from '../lib/format';
  import { app } from '../lib/state.svelte';

  let { id, running }: { id: string; running: boolean } = $props();

  const SHOWN = 100;

  let worlds = $state<World[]>([]);
  let canJoin = $state(false);
  let servers = $state<SavedServer[]>([]);
  let statuses = $state<Record<string, ServerStatus>>({});
  let loaded = $state(false);
  let query = $state('');
  let limit = $state(SHOWN);
  let open = $state<string | null>(null);
  let packs = $state<LockedMod[]>([]);
  let editing = $state<number | 'new' | null>(null);
  let name = $state('');
  let address = $state('');

  const matching = $derived.by(() => {
    const text = query.trim().toLowerCase();
    return text ? worlds.filter((world) => world.name.toLowerCase().includes(text)) : worlds;
  });

  async function load() {
    try {
      const found = await api.worlds(id);
      worlds = found.worlds;
      canJoin = found.canJoin;
      servers = await api.servers(id);
    } catch (error) {
      app.fail(error, 'Could not read this instance');
    }
    loaded = true;
    for (const server of servers) ask(server.address);
  }

  async function ask(target: string) {
    const status = await api.serverStatus(target).catch(() => null);
    if (status) statuses[target] = status;
  }

  $effect(() => {
    app.contentTick;
    load();
  });

  async function toggleWorld(world: World) {
    open = open === world.folder ? null : world.folder;
    if (open) packs = await api.dataPacks(id, open).catch(() => []);
  }

  async function setPack(pack: LockedMod, enabled: boolean) {
    try {
      await api.setContentEnabled(id, 'datapack', pack.hash, enabled, pack.group);
      packs = await api.dataPacks(id, pack.group!);
    } catch (error) {
      app.fail(error);
    }
  }

  async function removePack(pack: LockedMod) {
    try {
      await api.removeContent(id, 'datapack', [pack.hash], pack.group);
      packs = await api.dataPacks(id, pack.group!);
    } catch (error) {
      app.fail(error);
    }
  }

  async function deleteWorld(world: World) {
    try {
      const item = await api.deleteWorld(id, world.folder);
      app.contentTick++;
      app.toast('good', `Deleted ${world.name}`, 'You can restore it from Settings for 30 days.', { label: 'Undo', run: () => app.restore(item.id) });
    } catch (error) {
      app.fail(error, 'Could not delete that world');
    }
  }

  async function backUp(world: World) {
    try {
      const folder = await api.backUpWorld(id, world.folder);
      app.toast('good', `Backed up ${world.name}`, undefined, { label: 'Open folder', run: () => openFolder(folder) });
    } catch (error) {
      app.fail(error, 'Could not back up that world');
    }
  }

  function startEditing(target: SavedServer | 'new') {
    editing = target === 'new' ? 'new' : target.index;
    name = target === 'new' ? '' : target.name;
    address = target === 'new' ? '' : target.address;
  }

  async function saveServer(event: SubmitEvent) {
    event.preventDefault();
    const [title, target] = [name.trim() || address.trim(), address.trim()];
    try {
      servers = editing === 'new' ? await api.addServer(id, title, target) : await api.editServer(id, editing as number, title, target);
      editing = null;
      ask(target);
    } catch (error) {
      app.fail(error, 'Could not save that server');
    }
  }

  async function removeServer(server: SavedServer) {
    try {
      servers = await api.removeServer(id, server.index);
    } catch (error) {
      app.fail(error, 'Could not remove that server');
    }
  }

  const when = (world: World) => (world.lastPlayed ? `Played ${timeAgo(new Date(world.lastPlayed).toISOString())}` : 'Not played yet');
  const picture = (icon: string) => (icon.startsWith('data:') ? icon : `data:image/png;base64,${icon}`);

  function facts(status: ServerStatus | undefined): string {
    if (!status) return 'Checking';
    if (!status.online) return 'Offline';
    const players = status.players === undefined || status.players === null ? '' : `${status.players} of ${status.capacity ?? '?'} players`;
    return [players, status.latencyMs ? `${status.latencyMs} ms` : '', status.version ?? ''].filter(Boolean).join(' · ');
  }
</script>

<div class="tab">
  <section>
    <div class="head">
      <h2>Worlds</h2>
      {#if worlds.length > 6}<label class="search-box"><Search size={16} /><input class="field" placeholder="Search {worlds.length} worlds" aria-label="Search worlds" bind:value={query} /></label>{/if}
      <span class="spacer"></span>
      <button class="btn" onclick={() => app.go('discover', id, 'datapack')}><Compass size={16} /> Browse data packs</button>
    </div>

    {#if loaded && !worlds.length}
      <p class="muted none">This instance has no worlds yet.</p>
    {:else if loaded && !canJoin}
      <p class="faint small note">Opening a world directly needs Minecraft 1.20 or newer, so use Play on this instance and pick it in the game.</p>
    {/if}

    <ul>
      {#each matching.slice(0, limit) as world (world.folder)}
        <li class="world">
          <div class="row">
            <button class="fold" aria-expanded={open === world.folder} aria-label="Data packs in {world.name}" onclick={() => toggleWorld(world)}>
              {#if open === world.folder}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
            </button>
            <span class="icon">{#if world.hasIcon}<img src={localImage(`world/${id}/${world.folder}`)} alt="" loading="lazy" />{:else}{world.name.charAt(0).toUpperCase()}{/if}</span>
            <span class="text">
              <span class="name truncate selectable">{world.name}</span>
              <span class="sub truncate">{world.mode}{world.version ? `, ${world.version}` : ''}. {when(world)}.</span>
            </span>
            <span class="tools">
              <button class="btn" onclick={() => backUp(world)}>Back up</button>
              <button class="icon-button" aria-label="Delete {world.name}" title="Delete" disabled={running} onclick={() => deleteWorld(world)}><Trash2 size={15} /></button>
            </span>
            {#if canJoin}<button class="btn play" disabled={running} onclick={() => app.play(id, { kind: 'world', folder: world.folder })}><Play size={13} fill="currentColor" /> Play</button>{/if}
          </div>
          {#if open === world.folder}
            <div class="packs">
              {#each packs as pack (pack.hash)}
                <div class="pack" class:off={!pack.enabled}>
                  <input type="checkbox" checked={pack.enabled} aria-label="{pack.name} enabled" onchange={(event) => setPack(pack, event.currentTarget.checked)} />
                  <span class="small-icon">{#if iconUrl(pack.icon)}<img src={iconUrl(pack.icon)} alt="" loading="lazy" />{:else}{pack.name.charAt(0).toUpperCase()}{/if}</span>
                  <button class="truncate pack-name" onclick={() => app.openMod(id, pack)}>{pack.name}</button>
                  <span class="faint tabular">{pack.size ? bytes(pack.size) : 'Folder'}</span>
                  <button class="icon-button" aria-label="Remove {pack.name}" title="Remove" onclick={() => removePack(pack)}><X size={15} /></button>
                </div>
              {:else}
                <p class="faint">This world has no data packs.</p>
              {/each}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if matching.length > limit}
      <div class="more"><button class="btn" onclick={() => (limit += SHOWN)}>Show more</button></div>
    {/if}
  </section>

  <section>
    <div class="head">
      <h2>Servers</h2>
      <span class="spacer"></span>
      <button class="btn" onclick={() => startEditing('new')}><Plus size={16} /> Add a server</button>
    </div>

    {#if editing !== null}
      <form class="editor" onsubmit={saveServer}>
        <input class="field" placeholder="Name" aria-label="Server name" bind:value={name} />
        <input class="field" placeholder="Address, like play.example.com" aria-label="Server address" bind:value={address} />
        <button class="btn primary" disabled={!address.trim()}>Save</button>
        <button type="button" class="btn" onclick={() => (editing = null)}>Cancel</button>
      </form>
    {/if}

    {#if loaded && !servers.length && editing === null}
      <p class="muted none">No servers saved in this instance.</p>
    {/if}

    <ul>
      {#each servers as server (server.index)}
        {@const status = statuses[server.address]}
        {@const image = status?.icon ?? server.icon}
        <li class="server">
          <div class="row">
            <span class="fold"></span>
            <span class="icon">{#if image}<img src={picture(image)} alt="" />{:else}{server.name.charAt(0).toUpperCase()}{/if}</span>
            <span class="text">
              <span class="name truncate selectable">{server.name}</span>
              <span class="sub truncate" class:online={status?.online}>{facts(status)}</span>
              {#if status?.motd}<span class="motd truncate selectable">{status.motd}</span>{/if}
            </span>
            <span class="tools">
              <button class="icon-button" aria-label="Edit {server.name}" title="Edit" onclick={() => startEditing(server)}><Pencil size={14} /></button>
              <button class="icon-button" aria-label="Remove {server.name}" title="Remove" onclick={() => removeServer(server)}><X size={15} /></button>
            </span>
            <button class="btn play" disabled={running} onclick={() => app.play(id, { kind: 'server', address: server.address })}>Play</button>
          </div>
        </li>
      {/each}
    </ul>
  </section>
</div>

<style>
  .tab { height: 100%; overflow-y: auto; padding: 16px var(--gutter) var(--gutter); }
  section + section { margin-top: 22px; }
  .head { display: flex; align-items: center; gap: 10px; min-height: 36px; margin-bottom: 10px; }
  h2 { font-size: var(--text-l); }
  .search-box { flex: 0 1 240px; }
  .spacer { flex: 1; }
  .none { padding: 4px 0 8px; }
  .note { margin-bottom: 10px; }
  ul { box-shadow: inset 0 1px var(--line); }
  li { box-shadow: inset 0 -1px var(--line); content-visibility: auto; contain-intrinsic-size: auto 64px; }
  li:empty { display: none; }
  .row { display: grid; grid-template-columns: 20px 44px minmax(0, 1fr) auto auto; gap: 10px; align-items: center; min-height: 64px; padding: 8px 0; transition: background-color 80ms ease-out; }
  .row:hover { background: var(--selected); }
  .fold { display: grid; place-items: center; height: 44px; color: var(--text-3); }
  button.fold:hover { color: var(--text); }
  .icon {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    overflow: hidden;
    background: var(--control);
    clip-path: var(--stepped);
    color: var(--text-3);
    font-size: var(--text-l);
    font-weight: 600;
    image-rendering: pixelated;
  }
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .text { display: flex; flex-direction: column; min-width: 0; line-height: 1.35; }
  .name { font-size: var(--text-m); font-weight: 600; }
  .sub { color: var(--text-3); font-size: var(--text-xs); }
  .sub.online { color: var(--good); }
  .motd { color: var(--text-2); font-size: var(--text-xs); }
  .tools { display: flex; align-items: center; gap: 4px; opacity: 0; }
  .row:hover .tools, .tools:focus-within { opacity: 1; }
  .icon-button { display: grid; place-items: center; width: 32px; height: 32px; color: var(--text-3); }
  .icon-button:hover:not(:disabled) { color: var(--text); }
  .icon-button:disabled { cursor: not-allowed; }
  .play { min-width: 84px; }
  .packs { display: flex; flex-direction: column; gap: 2px; padding: 0 12px 12px 42px; }
  .pack { display: grid; grid-template-columns: 22px 28px minmax(0, 1fr) auto 28px; gap: 8px; align-items: center; height: 38px; }
  .pack.off .pack-name { opacity: 0.5; }
  .pack-name { text-align: left; }
  .pack-name:hover { text-decoration: underline; text-underline-offset: 2px; }
  .small-icon { display: grid; place-items: center; width: 28px; height: 28px; overflow: hidden; background: var(--control); clip-path: var(--stepped); color: var(--text-3); font-weight: 600; }
  .small-icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .editor { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.4fr) auto auto; gap: 8px; margin-bottom: 10px; }
  .more { padding: 12px 0; }
</style>

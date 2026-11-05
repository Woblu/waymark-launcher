<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import type { ContentHit, ContentKind, ContentSource, SearchQuery, Settings, Starter } from '../lib/api';
  import { EVERY_VERSION } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  let { settings = $bindable(), onsave, onclose }: { settings: Settings; onsave: () => Promise<void>; onclose: () => void } = $props();

  const KINDS: { id: ContentKind; label: string; one: string }[] = [
    { id: 'mod', label: 'Mods', one: 'Mod' },
    { id: 'resourcepack', label: 'Resource packs', one: 'Resource pack' },
    { id: 'shader', label: 'Shaders', one: 'Shader' },
  ];
  const STORES: Record<string, string> = { modrinth: 'Modrinth', curseforge: 'CurseForge' };

  let releases = $state<string[]>([]);
  let version = $state(EVERY_VERSION);
  let kind = $state<ContentKind>('mod');
  let source = $state<ContentSource>('modrinth');
  let hasCurseForge = $state(false);
  let text = $state('');
  let hits = $state<ContentHit[]>([]);
  let searching = $state(true);
  let problem = $state('');

  const list = $derived(settings.starters?.[version] ?? []);
  const count = (key: string) => settings.starters?.[key]?.length ?? 0;
  const chosen = (hit: ContentHit) => list.some((item) => item.source === hit.source && item.project === hit.id);
  const kindName = (item: Starter) => KINDS.find((entry) => entry.id === item.kind)?.one ?? '';

  onMount(async () => {
    const [manifest, curse] = await Promise.allSettled([api.minecraftVersions(), api.curseforgeAvailable()]);
    if (manifest.status === 'fulfilled') {
      releases = manifest.value.versions.filter((entry) => (entry.era ?? entry.kind) === 'release').map((entry) => entry.id);
      // Open on a list the player already has, or else the newest release.
      version = releases.find((id) => count(id)) ?? (count(EVERY_VERSION) ? EVERY_VERSION : (releases[0] ?? EVERY_VERSION));
    }
    if (curse.status === 'fulfilled') hasCurseForge = curse.value;
  });

  // An empty search shows the most downloaded, so there's something to pick from at once.
  $effect(() => {
    const words = text.trim();
    const query: SearchQuery = {
      text: words,
      kind,
      minecraft: version === EVERY_VERSION ? undefined : version,
      sort: words ? 'relevance' : 'downloads',
      offset: 0,
      limit: 8,
    };
    const from = source;
    let current = true;
    searching = true;
    const timer = setTimeout(async () => {
      try {
        const page = await api.search(from, query);
        if (!current) return;
        hits = page.hits;
        problem = '';
      } catch (error) {
        if (!current) return;
        hits = [];
        problem = (error as { message?: string })?.message ?? String(error);
      }
      searching = false;
    }, 250);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  });

  async function add(hit: ContentHit) {
    const item: Starter = { source: hit.source, project: hit.id, kind: hit.kind ?? kind, title: hit.title, icon: hit.icon };
    settings.starters = { ...settings.starters, [version]: [...list, item] };
    await onsave();
  }

  async function remove(item: Starter) {
    const rest = list.filter((other) => !(other.source === item.source && other.project === item.project));
    const next = { ...settings.starters };
    if (rest.length) next[version] = rest;
    else delete next[version];
    settings.starters = next;
    await onsave();
  }
</script>

<Modal title="Starter mods" width={600} {onclose}>
  <p class="muted intro">Added to each new instance on a Minecraft version. Waymark Launcher picks the build for the instance's mod loader and leaves out anything that doesn't have one.</p>
  <div class="form-row">
    <label for="starter-version">Minecraft version</label>
    <select id="starter-version" class="field" bind:value={version}>
      <option value={EVERY_VERSION}>{count(EVERY_VERSION) ? `Every version (${count(EVERY_VERSION)})` : 'Every version'}</option>
      {#each releases as id (id)}<option value={id}>{count(id) ? `${id} (${count(id)})` : id}</option>{/each}
    </select>
  </div>

  <ul class="list chosen" aria-label="Starter mods for this version">
    {#each list as item (item.source + item.project)}
      <li>
        <span class="icon">{#if item.icon}<img src={item.icon} alt="" loading="lazy" />{:else}{item.title.charAt(0).toUpperCase()}{/if}</span>
        <span class="what"><span class="title truncate">{item.title}</span><span class="faint small">{kindName(item)}</span></span>
        <button class="btn" onclick={() => remove(item)}>Remove</button>
      </li>
    {:else}
      <li class="empty faint">{version === EVERY_VERSION ? 'Nothing is added to every version yet.' : `Nothing is added to ${version} yet.`}</li>
    {/each}
  </ul>

  <div class="finder">
    <select class="field" aria-label="Kind of content" bind:value={kind}>
      {#each KINDS as entry (entry.id)}<option value={entry.id}>{entry.label}</option>{/each}
    </select>
    {#if hasCurseForge}
      <select class="field" aria-label="Store" bind:value={source}>
        {#each Object.entries(STORES) as [id, name] (id)}<option value={id}>{name}</option>{/each}
      </select>
    {/if}
    <input class="field search-starters" placeholder="Search" aria-label="Search" bind:value={text} />
  </div>
  <ul class="list hits" aria-busy={searching}>
    {#each hits as hit (hit.source + hit.id)}
      <li>
        <span class="icon">{#if hit.icon}<img src={hit.icon} alt="" loading="lazy" />{:else}{hit.title.charAt(0).toUpperCase()}{/if}</span>
        <span class="what"><span class="title truncate">{hit.title}</span><span class="faint small truncate selectable">{hit.summary}</span></span>
        {#if chosen(hit)}
          <button class="btn" disabled>Added</button>
        {:else}
          <button class="btn add-starter" onclick={() => add(hit)}>Add</button>
        {/if}
      </li>
    {:else}
      <li class="empty faint">{searching ? 'Searching' : problem ? `Could not search ${STORES[source]}` : 'Nothing found.'}</li>
    {/each}
  </ul>

  <div class="actions">
    <button class="btn primary" onclick={onclose}>Done</button>
  </div>
</Modal>

<style>
  .intro { margin: 0 0 10px; }
  .list { border-top: 1px solid var(--line); overflow-y: auto; }
  .chosen { max-height: 210px; margin-bottom: 14px; }
  .hits { max-height: 300px; }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
  }
  li.empty { padding: 12px 0; }
  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 32px;
    height: 32px;
    overflow: hidden;
    background: var(--control);
    clip-path: var(--stepped);
    color: var(--text-3);
    font-weight: 600;
  }
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .what { display: flex; flex: 1; flex-direction: column; min-width: 0; }
  .title { font-weight: 500; }
  .finder { display: flex; gap: 8px; margin-bottom: 8px; }
  .finder select { flex: none; width: auto; }
  .finder input { flex: 1; min-width: 0; }
  .actions { display: flex; justify-content: flex-end; padding-top: 12px; }
</style>

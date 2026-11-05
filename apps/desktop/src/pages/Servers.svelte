<script lang="ts">
  import { onMount } from 'svelte';
  import { Bookmark, BookmarkCheck, Play, Search } from '@lucide/svelte';
  import Art from '../components/Art.svelte';
  import JoinServer from '../components/JoinServer.svelte';
  import OfflineLine from '../components/OfflineLine.svelte';
  import Pager from '../components/Pager.svelte';
  import { api, inTauri } from '../lib/api';
  import type { KeptServer, KnownServer, PublicServer, SearchCategory, ServerKind, ServerSort, ServerStatus } from '../lib/api';
  import { asSentence, explain } from '../lib/errors';
  import type { Explained } from '../lib/errors';
  import { number, titleCase } from '../lib/format';
  import { tabKeys } from '../lib/tabs';
  import { GROUP_NAMES, GROUP_ORDER, LANGUAGE_IDS, REGION_IDS, categoryName, contentLine, languageName, linkedInstance, playersLine, regionName, serverPicture } from '../lib/servers';
  import type { ServerPreview } from '../lib/servers';
  import { app } from '../lib/state.svelte';
  import ServerPage from './ServerPage.svelte';

  const PAGE_SIZE = 24;
  const SORTS: { id: ServerSort; label: string }[] = [
    { id: 'popular', label: 'Popular' },
    { id: 'followed', label: 'Most followed' },
    { id: 'newest', label: 'Newest' },
    { id: 'updated', label: 'Recently updated' },
  ];

  let tab = $state<'browse' | 'yours'>('browse');
  let text = $state('');
  let sort = $state<ServerSort>('popular');
  let kind = $state<'' | ServerKind>('');
  let region = $state('');
  let language = $state('');
  let chosen = $state<string[]>([]);
  let categories = $state<SearchCategory[]>([]);

  let servers = $state<PublicServer[]>([]);
  let total = $state(0);
  let page = $state(1);
  let loading = $state(true);
  let problem = $state<Explained | null>(null);
  let results = $state<HTMLElement>();

  let kept = $state<KeptServer[]>([]);
  let known = $state<KnownServer[]>([]);
  let statuses = $state<Record<string, ServerStatus | null>>({});
  let playIn = $state<Record<string, string>>({});

  let opened = $state<ServerPreview | null>(null);
  let confirming = $state<PublicServer | null>(null);

  const pages = $derived(Math.max(1, Math.ceil(total / PAGE_SIZE)));
  const filtered = $derived(!!kind || !!region || !!language || chosen.length > 0);
  const groups = $derived(
    GROUP_ORDER.map((group) => [group, categories.filter((category) => category.group === group)] as const).filter(([, list]) => list.length),
  );
  const isKept = (id: string) => kept.some((server) => server.id === id);

  async function run(number: number) {
    page = number;
    loading = true;
    try {
      const found = await api.publicServers({
        text: text.trim(),
        sort,
        kind: kind || undefined,
        categories: chosen,
        region: region || undefined,
        language: language || undefined,
        offset: (number - 1) * PAGE_SIZE,
        limit: PAGE_SIZE,
      });
      servers = found.servers;
      total = found.total;
      // The browser preview opens the first server's page for screenshots.
      if (!inTauri && !opened && location.hash === '#server') opened = servers[0] ?? null;
      problem = null;
      app.reached();
      results?.scrollTo({ top: 0 });
    } catch (error) {
      servers = [];
      total = 0;
      problem = explain(error);
      if (problem.code === 'network') app.unreachable = true;
    }
    loading = false;
  }

  // Typing waits for a pause, and every other change searches at once from the first page.
  $effect(() => {
    const words = text;
    void [sort, kind, region, language, chosen.length];
    const timer = setTimeout(() => run(1), words.trim() ? 300 : 0);
    return () => clearTimeout(timer);
  });

  onMount(async () => {
    const [found, saved] = await Promise.allSettled([api.serverCategories(), api.keptServers()]);
    if (found.status === 'fulfilled') categories = found.value;
    if (saved.status === 'fulfilled') kept = saved.value;
  });

  /** Your servers are pinged from here each time the tab opens, a few at a time. */
  async function loadYours() {
    const [saved, inInstances] = await Promise.allSettled([api.keptServers(), api.knownServers()]);
    if (saved.status === 'fulfilled') kept = saved.value;
    if (inInstances.status === 'fulfilled') known = inInstances.value;
    const addresses = [...new Set([...kept.map((server) => server.address), ...known.map((server) => server.address)])];
    for (const address of addresses) if (!(address in statuses)) statuses[address] = null;
    const queue = [...addresses];
    const worker = async () => {
      for (let address = queue.shift(); address; address = queue.shift()) {
        statuses[address] = await api.serverStatus(address).catch(() => ({ online: false, motd: '' }) as ServerStatus);
      }
    };
    await Promise.all(Array.from({ length: 4 }, worker));
  }

  $effect(() => {
    if (tab === 'yours') loadYours();
  });

  function toggleCategory(id: string, on: boolean) {
    chosen = on ? [...chosen, id] : chosen.filter((other) => other !== id);
  }

  function clearFilters() {
    kind = '';
    region = '';
    language = '';
    chosen = [];
  }

  function join(server: PublicServer) {
    if (linkedInstance(app.library, server.id) || server.content.kind === 'vanilla') app.joinServer(server);
    else confirming = server;
  }

  async function keep(server: ServerPreview, on: boolean) {
    try {
      kept = await api.keepServer({ id: server.id, name: server.name, address: server.address, icon: server.icon ?? null }, on);
    } catch (error) {
      app.fail(error, 'Could not save that server');
    }
  }

  function statusText(status: ServerStatus | null | undefined): string {
    if (status === undefined || status === null) return 'Checking';
    if (!status.online) return 'Offline';
    if (status.players == null) return 'Online';
    return status.capacity ? `${number(status.players)} of ${number(status.capacity)} online` : `${number(status.players)} online`;
  }
</script>

{#if opened}
  {#key opened.id}
    <ServerPage preview={opened} kept={isKept(opened.id)} onkeep={keep} onback={() => (opened = null)} />
  {/key}
{:else}
  <div class="page">
    <div class="toolbar">
      <div class="tabs" role="tablist" aria-label="Server lists" use:tabKeys>
        <button role="tab" aria-selected={tab === 'browse'} tabindex={tab === 'browse' ? 0 : -1} class:active={tab === 'browse'} onclick={() => (tab = 'browse')}>Browse</button>
        <button role="tab" aria-selected={tab === 'yours'} tabindex={tab === 'yours' ? 0 : -1} class:active={tab === 'yours'} onclick={() => (tab = 'yours')}>Your servers{#if kept.length}<span class="count faint tabular">{kept.length}</span>{/if}</button>
      </div>
      {#if tab === 'browse'}
        <div class="filters">
          <label class="search-box wide"><Search size={16} /><input class="field search" placeholder="Search servers" aria-label="Search servers" bind:value={text} /></label>
          <label for="server-sort" class="muted">Sort</label>
          <select id="server-sort" class="field sort" bind:value={sort} disabled={!!text.trim()}>
            {#each SORTS as item (item.id)}<option value={item.id}>{item.label}</option>{/each}
          </select>
        </div>
      {/if}
    </div>

    {#if tab === 'browse'}
      <div class="body">
        <section class="side" aria-label="Filters">
          <fieldset>
            <legend class="heading">Type</legend>
            <label class="check"><input type="radio" name="server-kind" value="" bind:group={kind} /> All servers</label>
            <label class="check"><input type="radio" name="server-kind" value="vanilla" bind:group={kind} /> Vanilla</label>
            <label class="check"><input type="radio" name="server-kind" value="modded" bind:group={kind} /> Modded</label>
          </fieldset>
          <div class="choice">
            <label for="server-region" class="heading">Region</label>
            <select id="server-region" class="field" bind:value={region}>
              <option value="">Anywhere</option>
              {#each REGION_IDS as id (id)}<option value={id}>{regionName(id)}</option>{/each}
            </select>
          </div>
          <div class="choice">
            <label for="server-language" class="heading">Language</label>
            <select id="server-language" class="field" bind:value={language}>
              <option value="">Any language</option>
              {#each LANGUAGE_IDS as id (id)}<option value={id}>{languageName(id)}</option>{/each}
            </select>
          </div>
          {#each groups as [group, list] (group)}
            <fieldset>
              <legend class="heading">{GROUP_NAMES[group] ?? titleCase(group)}</legend>
              {#each list as category (category.id)}
                <label class="check">
                  <input type="checkbox" checked={chosen.includes(category.id)} onchange={(event) => toggleCategory(category.id, event.currentTarget.checked)} />
                  <span class="truncate">{categoryName(category.name)}</span>
                </label>
              {/each}
            </fieldset>
          {/each}
          {#if filtered}<button class="clear" onclick={clearFilters}>Clear filters</button>{/if}
        </section>

        <div class="results" class:loading bind:this={results}>
          {#if app.offline}
            <OfflineLine what="The server list comes from Modrinth, so it needs a connection." onretry={() => run(page)} />
          {/if}
          {#if servers.length}
            <div class="found">
              <span class="faint tabular">{`${number(total)} servers`}</span>
              <Pager {page} {pages} onpage={run} />
            </div>
          {/if}
          <ul class="grid">
            {#each servers as server (server.id)}
              {@const linked = linkedInstance(app.library, server.id)}
              <li>
                <button class="open focus-inset" onclick={() => (opened = server)}>
                  <span class="banner">{#if server.banner}<img src={server.banner} alt="" loading="lazy" referrerpolicy="no-referrer" />{:else}<Art seed={server.name} width={150} height={68} />{/if}</span>
                  <span class="icon">{#if server.icon}<img src={server.icon} alt="" loading="lazy" />{:else}{server.name.charAt(0).toUpperCase()}{/if}</span>
                  <span class="info">
                    <span class="title truncate">{server.name}</span>
                    <span class="faint by truncate">{contentLine(server.content, server.name)}</span>
                    <span class="muted summary">{server.summary}</span>
                  </span>
                </button>
                <div class="foot">
                  <span class="facts">
                    <span class="status tabular" class:on={server.online}><span class="mark"></span>{playersLine(server)}</span>
                    {#if server.region}<span class="faint region">{regionName(server.region)}</span>{/if}
                  </span>
                  <span class="buttons">
                    <button class="btn icon-only" aria-label={isKept(server.id) ? `Remove ${server.name} from your servers` : `Save ${server.name}`} title={isKept(server.id) ? 'Saved' : 'Save'} onclick={() => keep(server, !isKept(server.id))}>
                      {#if isKept(server.id)}<BookmarkCheck size={16} />{:else}<Bookmark size={16} />{/if}
                    </button>
                    <button class="btn add" disabled={!!app.joiningServer} onclick={() => join(server)}>
                      <Play size={14} fill="currentColor" /> {app.joiningServer === server.id ? 'Getting ready' : linked ? 'Play' : 'Join'}
                    </button>
                  </span>
                </div>
              </li>
            {/each}
          </ul>
          {#if !loading && problem && !app.offline}
            <div class="message">
              <p>The server list didn't load.</p>
              <p class="muted">{problem.sentence || asSentence(problem.technical)}</p>
              {#if !problem.complete && problem.sentence}<p class="faint small selectable mono">{problem.technical}</p>{/if}
              <button class="btn" onclick={() => run(page)}>Try again</button>
            </div>
          {:else if !loading && !servers.length && !problem}
            <p class="none muted">No servers match.</p>
          {/if}
          {#if servers.length}
            <div class="bottom"><Pager {page} {pages} onpage={run} /></div>
          {/if}
          <p class="credit faint">Servers are listed by their owners on Modrinth.</p>
        </div>
      </div>
    {:else}
      <div class="yours">
        <section>
          <h2>Saved</h2>
          {#each kept as server (server.id)}
            {@const linked = linkedInstance(app.library, server.id)}
            <div class="row">
              <button class="who" onclick={() => (opened = server)}>
                <span class="small-icon">{#if server.icon}<img src={server.icon} alt="" />{:else}{server.name.charAt(0).toUpperCase()}{/if}</span>
                <span class="names">
                  <span class="name truncate">{server.name}</span>
                  <span class="faint mono truncate">{server.address}</span>
                </span>
              </button>
              <span class="status tabular" class:on={statuses[server.address]?.online}><span class="mark"></span>{statusText(statuses[server.address])}</span>
              <button class="btn" onclick={() => keep(server, false)}>Remove</button>
              {#if linked}
                <button class="btn primary" disabled={!!app.joiningServer} onclick={() => app.joinServer(server)}><Play size={14} fill="currentColor" /> {app.joiningServer === server.id ? 'Getting ready' : 'Play'}</button>
              {:else}
                <button class="btn primary" onclick={() => (opened = server)}>Join</button>
              {/if}
            </div>
          {:else}
            <p class="faint empty">Servers you save from Browse appear here.</p>
          {/each}
        </section>

        <section>
          <h2>In your instances</h2>
          {#each known as server (server.address)}
            {@const target = playIn[server.address] ?? server.instances[0]?.id}
            {@const running = app.library.find((entry) => entry.id === target)?.running ?? false}
            <div class="row">
              <span class="who static">
                <span class="small-icon">{#if statuses[server.address]?.icon ?? server.icon}<img src={serverPicture((statuses[server.address]?.icon ?? server.icon)!)} alt="" />{:else}{server.name.charAt(0).toUpperCase()}{/if}</span>
                <span class="names">
                  <span class="name truncate">{server.name}</span>
                  <span class="faint mono truncate">{server.address}</span>
                </span>
              </span>
              <span class="status tabular" class:on={statuses[server.address]?.online}><span class="mark"></span>{statusText(statuses[server.address])}</span>
              {#if server.instances.length > 1}
                <select class="field instance" aria-label={`Instance to play ${server.name} in`} value={target} onchange={(event) => (playIn[server.address] = event.currentTarget.value)}>
                  {#each server.instances as place (place.id)}<option value={place.id}>{place.name}</option>{/each}
                </select>
              {:else}
                <span class="faint instance truncate">{server.instances[0]?.name}</span>
              {/if}
              <button class="btn primary" disabled={!target || running} onclick={() => target && app.play(target, { kind: 'server', address: server.address })}><Play size={14} fill="currentColor" /> Play</button>
            </div>
          {:else}
            <p class="faint empty">Servers in your instances' multiplayer lists appear here.</p>
          {/each}
        </section>
      </div>
    {/if}
  </div>
{/if}

{#if confirming}<JoinServer server={confirming} onclose={() => (confirming = null)} />{/if}

<style>
  .page { display: flex; flex-direction: column; height: 100%; }
  .toolbar { flex: none; padding: 6px var(--gutter) 0; }
  .filters { display: flex; align-items: center; gap: 8px; padding: 14px 0; }
  .wide { flex: 1; min-width: 160px; }
  .sort { width: 170px; }
  .count { margin-left: 6px; }

  .body { flex: 1; min-height: 0; display: grid; grid-template-columns: 196px minmax(0, 1fr); gap: 20px; padding-left: var(--gutter); }
  .side { min-height: 0; overflow-y: auto; padding: 0 6px var(--gutter) 0; }
  .heading { display: block; margin-bottom: 6px; color: var(--text); font-weight: 600; }
  .choice { margin-bottom: 14px; }
  fieldset { min-width: 0; margin: 0 0 14px; padding: 0; border: 0; }
  legend { padding: 0; }
  .check { display: flex; align-items: center; gap: 9px; min-width: 0; padding: 4px 0; color: var(--text-2); cursor: pointer; }
  .check:hover { color: var(--text); }
  .clear { color: var(--text-2); text-decoration: underline; text-underline-offset: 2px; }
  .clear:hover { color: var(--text); }

  .results { min-height: 0; overflow-y: auto; padding: 0 16px var(--gutter) 0; }
  .results.loading .grid { opacity: 0.6; }
  .found { display: flex; align-items: center; justify-content: space-between; min-height: 34px; margin-bottom: 10px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 14px; }
  .grid li {
    --step: 3px;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: var(--panel);
    clip-path: var(--stepped);
    content-visibility: auto;
    contain-intrinsic-size: auto 300px;
    transition: background-color 100ms ease-out;
  }
  .grid li:hover { background: var(--raised); }
  .open { position: relative; display: flex; flex: 1; flex-direction: column; min-width: 0; text-align: left; }
  .banner { display: block; overflow: hidden; aspect-ratio: 11 / 5; background: var(--control); }
  .banner img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .icon {
    position: absolute;
    top: 0;
    left: 12px;
    display: grid;
    place-items: center;
    width: 52px;
    height: 52px;
    margin-top: calc((100% * 5 / 11) - 34px);
    overflow: hidden;
    background: var(--control);
    clip-path: var(--stepped);
    color: var(--text-3);
    font-size: var(--text-xl);
    font-weight: 600;
  }
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .info { display: flex; flex-direction: column; min-width: 0; padding: 26px 12px 0; }
  .title { font-size: var(--text-l); font-weight: 700; letter-spacing: -0.01em; }
  .open:hover .title { text-decoration: underline; text-underline-offset: 2px; }
  .by { font-size: var(--text-xs); }
  .summary {
    display: -webkit-box;
    overflow: hidden;
    margin-top: 5px;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
  .foot { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 58px; padding: 10px 12px 12px; }
  .facts { display: flex; flex-direction: column; gap: 2px; min-width: 0; font-size: var(--text-xs); }
  .region { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .buttons { display: flex; flex: none; gap: 6px; }
  .icon-only { width: 36px; padding: 0; justify-content: center; }

  .status { display: inline-flex; align-items: center; gap: 6px; min-width: 0; color: var(--text-3); white-space: nowrap; }
  .mark { flex: none; width: 7px; height: 7px; background: var(--text-3); }
  .status.on { color: var(--text-2); }
  .status.on .mark { background: var(--good); }

  .none { padding: 20px 0; }
  .message { display: flex; flex-direction: column; align-items: flex-start; gap: 4px; padding: 20px 0; }
  .message .btn { margin-top: 8px; }
  .bottom { display: flex; justify-content: center; padding-top: 16px; }
  .credit { padding-top: 18px; font-size: var(--text-xs); text-align: center; }

  .yours { flex: 1; min-height: 0; overflow-y: auto; padding: 6px var(--gutter) var(--gutter); }
  .yours section { max-width: 920px; margin-bottom: 26px; }
  .yours h2 { margin-bottom: 8px; font-size: var(--text-l); }
  .row { display: grid; grid-template-columns: minmax(0, 1fr) 170px auto auto; gap: 12px; align-items: center; min-height: 56px; padding: 6px 0; border-top: 1px solid var(--line); }
  .row:last-child { border-bottom: 1px solid var(--line); }
  .who { display: flex; align-items: center; gap: 10px; min-width: 0; text-align: left; }
  .who:not(.static):hover .name { text-decoration: underline; text-underline-offset: 2px; }
  .small-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    overflow: hidden;
    background: var(--control);
    clip-path: var(--stepped);
    color: var(--text-3);
    font-weight: 600;
  }
  .small-icon img { display: block; width: 100%; height: 100%; object-fit: cover; image-rendering: pixelated; }
  .names { display: flex; flex-direction: column; min-width: 0; line-height: 1.3; }
  .name { font-weight: 600; }
  .instance { width: 170px; }
  .empty { padding: 12px 0; border-top: 1px solid var(--line); }
</style>

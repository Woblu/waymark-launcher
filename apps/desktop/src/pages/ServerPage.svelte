<script lang="ts">
  import { onMount } from 'svelte';
  import { ArrowLeft, Bookmark, BookmarkCheck, Copy, ExternalLink, Play } from '@lucide/svelte';
  import Art from '../components/Art.svelte';
  import JoinServer from '../components/JoinServer.svelte';
  import Lightbox from '../components/Lightbox.svelte';
  import RichText from '../components/RichText.svelte';
  import { api, openExternal } from '../lib/api';
  import type { PublicServer, ServerDetails, ServerStatus, Shot } from '../lib/api';
  import { asSentence, explain } from '../lib/errors';
  import type { Explained } from '../lib/errors';
  import { count, number } from '../lib/format';
  import { categoryName, contentLine, languageName, linkedInstance, packLine, playersLine, regionName } from '../lib/servers';
  import type { ServerPreview } from '../lib/servers';
  import { app } from '../lib/state.svelte';

  let {
    preview,
    kept,
    onkeep,
    onback,
  }: { preview: ServerPreview; kept: boolean; onkeep: (server: ServerPreview, keep: boolean) => void; onback: () => void } = $props();

  const BACK_BUTTON = 3;

  let details = $state<ServerDetails | null>(null);
  let problem = $state<Explained | null>(null);
  let live = $state<ServerStatus | null>(null);
  let pinging = $state(true);
  let confirming = $state(false);
  let copied = $state(false);
  let viewing = $state<{ shots: Shot[]; start: number } | null>(null);

  const server = $derived<ServerPreview>(details ?? preview);
  const linked = $derived(linkedInstance(app.library, server.id));
  const shots = $derived<Shot[]>((details?.gallery ?? []).map((url) => ({ url, full: url })));
  const online = $derived(live ? live.online : (server.online ?? false));
  const versions = $derived(server.content?.kind === 'vanilla' ? server.content.versions : []);

  async function loadDetails() {
    problem = null;
    try {
      details = await api.publicServer(preview.id);
      app.reached();
    } catch (error) {
      problem = explain(error);
      if (problem.code === 'network') app.unreachable = true;
    }
  }

  onMount(async () => {
    api
      .serverStatus(preview.address)
      .then((status) => (live = status))
      .catch(() => {})
      .finally(() => (pinging = false));
    await loadDetails();
  });

  function join() {
    if (!details) return;
    if (linked || details.content.kind === 'vanilla') app.joinServer(details);
    else confirming = true;
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(server.address);
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch (error) {
      app.fail(error, 'Could not copy the address');
    }
  }

  function zoom(url: string) {
    const start = shots.findIndex((shot) => shot.url === url);
    viewing = { shots: start >= 0 ? shots : [{ url, full: url }], start: Math.max(start, 0) };
  }

  function onmouseup(event: MouseEvent) {
    if (event.button === BACK_BUTTON && !viewing && !confirming) onback();
  }

  const statusLine = $derived.by(() => {
    if (live?.online) {
      if (live.players == null) return 'Online';
      return live.capacity ? `${number(live.players)} of ${number(live.capacity)} online` : `${number(live.players)} online`;
    }
    if (live && !live.online) return 'Offline';
    return server.content ? playersLine(server as PublicServer) : '';
  });
</script>

<svelte:window {onmouseup} />

<div class="server">
  <div class="cover">
    {#if server.banner}<img src={server.banner} alt="" referrerpolicy="no-referrer" />{:else}<Art seed={server.name} width={480} height={100} />{/if}
    <button class="back" onclick={onback}><ArrowLeft size={16} /> Servers</button>
  </div>

  <header class="head">
    <span class="icon">{#if server.icon}<img src={server.icon} alt="" />{:else}{server.name.charAt(0).toUpperCase()}{/if}</span>
    <div class="about">
      <h1 class="title">{server.name}</h1>
      {#if server.summary}<p class="summary muted selectable">{server.summary}</p>{/if}
      <p class="stats faint tabular">
        {#if statusLine}<span class="status" class:on={online}><span class="mark"></span>{statusLine}</span>{/if}
        {#if server.content}<span>{contentLine(server.content, server.name)}</span>{/if}
        {#if server.region}<span>{regionName(server.region)}</span>{/if}
        {#if server.plays}<span>{`${count(server.plays)} joins in the last two weeks`}</span>{/if}
      </p>
    </div>
    <div class="actions">
      <button class="btn primary" disabled={!details || !!app.joiningServer} onclick={join}>
        <Play size={16} fill="currentColor" />
        {app.joiningServer === server.id ? 'Getting ready' : linked ? 'Play' : 'Join'}
      </button>
      <div class="row">
        <button class="btn" aria-pressed={kept} onclick={() => onkeep(server, !kept)}>
          {#if kept}<BookmarkCheck size={15} /> Saved{:else}<Bookmark size={15} /> Save{/if}
        </button>
        <button class="btn" onclick={copy}><Copy size={15} /> {copied ? 'Copied' : 'Copy address'}</button>
      </div>
      {#if linked}<p class="faint small">{`Plays in ${linked.name}`}</p>{/if}
    </div>
  </header>

  <div class="columns">
    <div class="main">
      {#if problem}
        <div class="note problem">
          <p>This server's page didn't load.</p>
          <p class="muted">{problem.sentence || asSentence(problem.technical)}</p>
          <button class="btn" onclick={loadDetails}>Try again</button>
        </div>
      {:else if !details}
        <p class="faint">Loading</p>
      {:else}
        {#if details.description}
          <RichText source={details.description} format="markdown" base={details.page} onimage={zoom} />
        {:else}
          <p class="faint">The owner hasn't written a description yet.</p>
        {/if}
        {#if shots.length}
          <h2 class="gallery-title">Gallery</h2>
          <ul class="gallery">
            {#each shots as shot, index (shot.url)}
              <li><button class="focus-inset" aria-label="View picture {index + 1}" onclick={() => (viewing = { shots, start: index })}><img src={shot.url} alt="" loading="lazy" referrerpolicy="no-referrer" /></button></li>
            {/each}
          </ul>
        {/if}
      {/if}
    </div>

    <aside>
      <section>
        <h2>Address</h2>
        <p class="mono selectable address">{server.address}</p>
      </section>
      <section>
        <h2>Right now</h2>
        {#if pinging}
          <p class="faint">Checking</p>
        {:else if live?.online}
          <dl>
            <dt>Players</dt><dd class="tabular">{live.players != null ? (live.capacity ? `${number(live.players)} of ${number(live.capacity)}` : number(live.players)) : 'Hidden'}</dd>
            {#if live.latencyMs != null}<dt>Ping</dt><dd class="tabular">{`${live.latencyMs} ms`}</dd>{/if}
            {#if live.version}<dt>Version</dt><dd class="selectable">{live.version}</dd>{/if}
          </dl>
          {#if live.motd}<p class="motd mono selectable">{live.motd}</p>{/if}
        {:else}
          <p class="faint">It didn't answer from here just now. It may be down, or busy.</p>
        {/if}
      </section>
      <section>
        <h2>Details</h2>
        <dl>
          {#if server.content?.kind === 'modpack'}
            <dt>Modpack</dt><dd>{server.content.name ?? server.name}</dd>
            <dt>Plays on</dt><dd>{packLine(server.content)}</dd>
          {:else if server.content}
            <dt>Plays on</dt><dd>{contentLine(server.content)}</dd>
          {/if}
          {#if versions.length > 1}<dt>Versions</dt><dd>{versions.join(', ')}</dd>{/if}
          {#if server.region}<dt>Region</dt><dd>{regionName(server.region)}</dd>{/if}
          {#if server.languages?.length}<dt>Languages</dt><dd>{server.languages.map(languageName).join(', ')}</dd>{/if}
          {#if server.categories?.length}<dt>Tags</dt><dd>{server.categories.map(categoryName).join(', ')}</dd>{/if}
          {#if details?.followers}<dt>Followers</dt><dd class="tabular">{count(details.followers)}</dd>{/if}
        </dl>
      </section>
      {#if details}
        <section>
          <h2>Links</h2>
          {#each details.links as link (link.url)}
            <button class="link" onclick={() => openExternal(link.url)}><ExternalLink size={14} /> {link.label}</button>
          {/each}
          <button class="link" onclick={() => openExternal(details!.page)}><ExternalLink size={14} /> Listing on Modrinth</button>
        </section>
      {/if}
    </aside>
  </div>
</div>

{#if confirming && details}<JoinServer server={details} onclose={() => (confirming = false)} />{/if}
{#if viewing}<Lightbox shots={viewing.shots} start={viewing.start} onclose={() => (viewing = null)} />{/if}

<style>
  .server { --side: clamp(256px, 24vw, 330px); height: 100%; overflow-y: auto; }
  .cover { position: relative; height: clamp(110px, 20vh, 170px); background: var(--control); }
  .cover img { display: block; width: 100%; height: 100%; object-fit: cover; object-position: center 35%; }
  .back {
    position: absolute;
    top: 12px;
    left: var(--gutter);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px 0 8px;
    background: rgb(8 10 14 / 70%);
    clip-path: var(--stepped);
    color: #f0ebe1;
    font-weight: 600;
  }
  .back:hover { background: rgb(8 10 14 / 85%); }
  /* The header sits wholly below the banner, so a busy picture never runs behind the name or the buttons. */
  .head {
    display: grid;
    grid-template-columns: 84px minmax(0, 1fr) var(--side);
    gap: 16px;
    align-items: start;
    padding: 18px var(--gutter);
  }
  .icon {
    display: grid;
    place-items: center;
    width: 84px;
    height: 84px;
    overflow: hidden;
    background: var(--control);
    clip-path: var(--stepped);
    color: var(--text-3);
    font-size: 32px;
    font-weight: 600;
  }
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; image-rendering: pixelated; }
  .about { min-width: 0; }
  h1 { font-size: 30px; letter-spacing: -0.025em; overflow-wrap: anywhere; }
  .summary { margin-top: 3px; font-size: var(--text-m); }
  .stats { display: flex; flex-wrap: wrap; margin-top: 5px; }
  .stats > span { white-space: nowrap; }
  .stats > span + span::before { content: ''; display: inline-block; width: 4px; height: 4px; margin: 0 8px 2px; background: var(--text-3); }
  .status { display: inline-flex; align-items: center; gap: 6px; }
  .mark { width: 7px; height: 7px; background: var(--text-3); }
  .status.on { color: var(--text-2); }
  .status.on .mark { background: var(--good); }

  .actions { display: flex; flex-direction: column; align-items: stretch; gap: 8px; }
  .actions .row { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .actions .btn { justify-content: center; }
  .actions p { text-align: right; }

  .columns { display: grid; grid-template-columns: minmax(0, 1fr) var(--side); gap: 32px; padding: 16px var(--gutter) 40px; border-top: 1px solid var(--line); }
  .main { min-width: 0; }
  .note { padding: 10px 14px; background: var(--panel); clip-path: var(--stepped); color: var(--text-2); }
  .problem { display: flex; flex-direction: column; align-items: flex-start; gap: 4px; color: var(--text); }
  .problem .btn { margin-top: 8px; }
  .gallery-title { margin: 24px 0 10px; }
  .gallery { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; }
  .gallery button { display: block; width: 100%; aspect-ratio: 16 / 9; overflow: hidden; background: var(--panel); clip-path: var(--stepped); cursor: zoom-in; }
  .gallery img { display: block; width: 100%; height: 100%; object-fit: cover; }

  aside { min-width: 0; }
  section { padding: 14px 0; border-top: 1px solid var(--line); }
  section:first-child { padding-top: 0; border-top: 0; }
  h2 { margin-bottom: 8px; font-size: var(--text-m); }
  .address { color: var(--text); overflow-wrap: anywhere; }
  dl { display: grid; grid-template-columns: 78px minmax(0, 1fr); gap: 5px 10px; }
  dt { color: var(--text-3); }
  dd { color: var(--text-2); overflow-wrap: anywhere; }
  .motd { margin-top: 10px; color: var(--text-2); font-size: var(--text-xs); white-space: pre-line; overflow-wrap: anywhere; }
  .link { display: flex; align-items: center; gap: 5px; padding: 3px 0; color: var(--text-2); }
  .link:hover { color: var(--text); }
</style>

<script lang="ts" module>
  import type { ProjectPage } from '../lib/api';

  /** Pages stay for the session, so going back doesn't reload them. */
  const pages = new Map<string, ProjectPage>();
</script>

<script lang="ts">
  import { untrack } from 'svelte';
  import { Download, ExternalLink } from '@lucide/svelte';
  import Art from '../components/Art.svelte';
  import Lightbox from '../components/Lightbox.svelte';
  import RichText from '../components/RichText.svelte';
  import { api, iconUrl, openExternal } from '../lib/api';
  import type { ContentSource, Fit, LocalMod, LockedMod, ProjectTarget, Shot, World } from '../lib/api';
  import { bytes, count, day, environment, loaderName, minecraftRange, timeAgo, titleCase } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import ProjectVersions from './ProjectVersions.svelte';

  let { target }: { target: ProjectTarget } = $props();

  type Tab = 'description' | 'gallery' | 'versions';
  const KIND_NAMES = { mod: 'Mod', modpack: 'Modpack', shader: 'Shader', resourcepack: 'Resource pack', datapack: 'Data pack' };
  const BACK_BUTTON = 3;
  const storeNames: Record<ContentSource, string> = { modrinth: 'Modrinth', curseforge: 'CurseForge', ftb: 'Feed The Beast', atlauncher: 'ATLauncher' };

  let page = $state<ProjectPage | null>(null);
  let local = $state<LocalMod | null>(null);
  let build = $state<{ file: string; version?: string } | null>(null);
  let failed = $state<string | null>(null);
  let tab = $state<Tab>('description');
  let instanceId = $state(untrack(() => target.instance ?? app.discoverTarget ?? app.target?.id ?? ''));
  let fit = $state<Fit | null | undefined>(undefined);
  let installed = $state<LockedMod | null>(null);
  let busy = $state(false);
  let viewing = $state<{ shots: Shot[]; start: number } | null>(null);
  let worlds = $state<World[]>([]);
  let world = $state('');

  const preview = $derived(target.preview);
  const instance = $derived(app.entry(instanceId));
  const title = $derived(page?.title ?? local?.name ?? preview?.title ?? '');
  const icon = $derived(page?.icon ?? iconUrl(local?.icon) ?? preview?.icon ?? null);
  const summary = $derived(page?.summary ?? preview?.summary ?? '');
  const storeName = $derived(storeNames[page?.source ?? 'modrinth']);
  const isDataPack = $derived(page?.kind === 'datapack');
  /** A data pack is installed per world, so the instance alone can't answer. */
  const inInstance = $derived(!!page && (isDataPack ? !!installed : app.isInstalled(page, instanceId)));
  const description = $derived(page && (page.body.trim() || page.summary));
  const environmentText = $derived(page && environment(page.client, page.server));

  async function load() {
    failed = null;
    try {
      let [source, id]: [ContentSource | undefined, string | undefined] = 'hash' in target ? [undefined, undefined] : [target.source, target.id];
      if ('hash' in target) {
        const found = await api.installedModPage(target.instance, target.hash);
        if (found.kind === 'local') {
          local = found;
          return;
        }
        if (found.kind === 'namesake') build = { file: found.file, version: found.version };
        [source, id] = [found.source, found.project];
      }
      const key = `${source}:${id}`;
      const loaded = pages.get(key) ?? (await api.projectPage(source!, id!));
      pages.set(key, loaded);
      // A data pack can also be published as a mod, so where it was opened from decides.
      page = 'kind' in target && target.kind ? { ...loaded, kind: target.kind } : loaded;
    } catch (error) {
      failed = (error as { message?: string })?.message ?? String(error);
    }
  }

  $effect(() => {
    untrack(load);
  });

  // What fits the chosen instance, and whether it's already there.
  $effect(() => {
    const project = page;
    const chosen = instanceId;
    app.contentTick;
    if (!project || !chosen || project.kind === 'modpack') return;
    app.loadInstalled(chosen);
    fit = undefined;
    let current = true;
    api.projectFit(chosen, project.source, project.id, project.kind).then(
      (found) => current && (fit = found),
      () => current && (fit = null),
    );
    const inWorld = world;
    api.content(chosen, project.kind).then((list) => {
      if (!current) return;
      const same = (entry: LockedMod) =>
        entry.source.kind === project.source && String(entry.source.project) === project.id && (project.kind !== 'datapack' || entry.group === inWorld);
      installed = list.find(same) ?? null;
    }, () => {});
    return () => (current = false);
  });

  $effect(() => {
    const chosen = instanceId;
    if (!isDataPack || !chosen) return;
    let current = true;
    api.worlds(chosen).then((found) => {
      if (!current) return;
      worlds = found.worlds;
      world = found.worlds[0]?.folder ?? '';
    }, () => {});
    return () => (current = false);
  });

  async function add(version?: string) {
    if (!page) return;
    busy = true;
    await app.install({ source: page.source, id: page.id, kind: page.kind, title: page.title }, instanceId, version, isDataPack ? world : undefined);
    busy = false;
  }

  async function switchTo(version: string) {
    if (!installed) return;
    busy = true;
    try {
      const mark = await api.applyUpdates(instanceId, [{ hash: installed.hash, version }]);
      if (mark) app.toast('good', mark.label, 'You can undo this from Waymarks.');
      app.contentTick++;
    } catch (error) {
      app.fail(error, 'Could not switch to that version');
    }
    busy = false;
  }

  function openSlug(slug: string) {
    app.openProject({ source: 'modrinth', id: slug, instance: instanceId || undefined });
  }

  function openNeeded(source: ContentSource, id: string, name: string, picture?: string | null) {
    app.openProject({ source, id, instance: instanceId || undefined, preview: { title: name, icon: picture } });
  }

  /** Images in a description join the gallery view when they're part of it. */
  function zoom(url: string, alt: string) {
    const shots = page?.gallery ?? [];
    const start = shots.findIndex((shot) => shot.url === url || shot.full === url);
    viewing = start >= 0 ? { shots, start } : { shots: [{ url, full: url, title: alt || undefined }], start: 0 };
  }

  function onkeydown(event: KeyboardEvent) {
    const back = event.key === 'Escape' || (event.altKey && event.key === 'ArrowLeft');
    if (back && !viewing) app.closeProject();
  }

  function onmouseup(event: MouseEvent) {
    if (event.button === BACK_BUTTON && !viewing) app.closeProject();
  }
</script>

<svelte:window {onkeydown} {onmouseup} />

<div class="project">
  <div class="scene cover">
    {#if page?.gallery.length}<img src={page.gallery[0].url} alt="" referrerpolicy="no-referrer" />{:else}<Art seed={title} width={480} height={100} />{/if}
  </div>

  <header class="head">
    <span class="icon">{#if icon}<img src={icon} alt="" />{:else}{title.charAt(0).toUpperCase()}{/if}</span>
    <div class="about">
      <h1>{title}</h1>
      {#if summary}<p class="summary muted selectable">{summary}</p>{/if}
      {#if page}
        <p class="stats faint tabular">
          <span>{KIND_NAMES[page.kind]}</span>
          {#if page.downloads}<span>{count(page.downloads)} downloads</span>{/if}
          {#if page.followers}<span>{count(page.followers)} followers</span>{/if}
          {#if page.updated}<span>Updated {timeAgo(page.updated)}</span>{/if}
        </p>
      {:else if local}
        <p class="stats faint">{local.version ?? 'Unknown version'}{#if local.authors.length} · By {local.authors.join(', ')}{/if}</p>
      {/if}
    </div>

    {#if page}
      <div class="actions">
        {#if page.kind === 'modpack'}
          <button class="btn primary" disabled={busy} onclick={() => add()}><Download size={16} /> {busy ? 'Installing' : 'Install as a new instance'}</button>
        {:else if build}
          <p class="have">Installed build {build.version ?? ''}</p>
        {:else if !app.library.length}
          <p class="faint small">Create an instance to install this.</p>
        {:else}
          <div class="target">
            <select class="field" aria-label="Instance" bind:value={instanceId}>
              {#each app.library as entry (entry.id)}<option value={entry.id}>{entry.name}</option>{/each}
            </select>
            {#if isDataPack && worlds.length}
              <select class="field" aria-label="World" bind:value={world}>
                {#each worlds as item (item.folder)}<option value={item.folder}>{item.name}</option>{/each}
              </select>
            {/if}
            {#if inInstance}
              <span class="have">Installed</span>
            {:else}
              <button class="btn primary" disabled={busy || !fit || (isDataPack && !world)} onclick={() => add()}><Download size={16} /> {busy ? 'Installing' : 'Install'}</button>
            {/if}
          </div>
          <p class="faint small fit">
            {#if isDataPack && !worlds.length}
              Data packs go in a world, and {instance?.name} has none yet
            {:else if installed}
              {installed.version ? `Version ${installed.version} is in this ${isDataPack ? 'world' : 'instance'}` : `Already in this ${isDataPack ? 'world' : 'instance'}`}
            {:else if inInstance}
              In {instance?.name}
            {:else if fit}
              Installs {fit.version}
            {:else if fit === null && instance}
              No version for {instance.minecraft} {loaderName(instance.loader.kind)}
            {:else}
              Checking versions
            {/if}
          </p>
        {/if}
        <button class="site" onclick={() => openExternal(page!.page)}>Open on {storeName} <ExternalLink size={12} /></button>
      </div>
    {/if}
  </header>

  {#if failed}
    <div class="message">
      <p>This page could not be loaded.</p>
      <p class="muted">{failed}</p>
      <button class="btn" onclick={load}>Try again</button>
    </div>
  {:else if local}
    <div class="columns">
      <div class="main">
        <p class="note">This mod didn't come from Modrinth or CurseForge, so this is what its own file says about it.</p>
        {#if local.description}<p class="plain selectable">{local.description}</p>{/if}
      </div>
      <aside>
        <section>
          <h2>Details</h2>
          <dl>
            <dt>File</dt><dd class="selectable">{local.file}</dd>
            <dt>Size</dt><dd>{bytes(local.size)}</dd>
            {#if local.license}<dt>License</dt><dd>{local.license}</dd>{/if}
          </dl>
        </section>
        {#if local.links.length}
          <section>
            <h2>Links</h2>
            {#each local.links as link (link.url)}
              <button class="link" title={link.url} onclick={() => openExternal(link.url)}>{link.label} <ExternalLink size={12} /></button>
            {/each}
          </section>
        {/if}
      </aside>
    </div>
  {:else if page}
    <div class="bar">
      <nav class="tabs" aria-label="Project">
        <button class:active={tab === 'description'} onclick={() => (tab = 'description')}>Description</button>
        {#if page.gallery.length}
          <button class:active={tab === 'gallery'} onclick={() => (tab = 'gallery')}>Gallery <span class="faint tabular">{page.gallery.length}</span></button>
        {/if}
        <button class:active={tab === 'versions'} onclick={() => (tab = 'versions')}>Versions</button>
      </nav>
    </div>

    <div class="columns" class:wide={tab === 'versions'}>
      <div class="main">
        {#if build}
          <p class="note">
            You have <span class="selectable">{build.file}</span>, a different build from the ones Modrinth lists. This page is about the original project,
            so its details and versions may not match your file.
          </p>
        {/if}
        {#if tab === 'description'}
          {#if description}
            <RichText source={description} format={page.format} base={page.page} onimage={zoom} onproject={openSlug} />
          {:else}
            <p class="muted">This project has no description.</p>
          {/if}
        {:else if tab === 'gallery'}
          <ul class="gallery">
            {#each page.gallery as shot, index (shot.url)}
              <li>
                <button aria-label={shot.title ?? `Image ${index + 1}`} onclick={() => (viewing = { shots: page!.gallery, start: index })}>
                  <img src={shot.url} alt="" loading="lazy" referrerpolicy="no-referrer" />
                </button>
                {#if shot.title}<p class="truncate">{shot.title}</p>{/if}
                {#if shot.description}<p class="muted small caption">{shot.description}</p>{/if}
              </li>
            {/each}
          </ul>
        {:else}
          <ProjectVersions {page} {instance} matched={!!fit} installed={installed ? (installed.version ?? '') : undefined} locked={!!build} {busy} onadd={add} onswitch={switchTo} onproject={openSlug} />
        {/if}
      </div>

      {#if tab !== 'versions'}
        <aside>
          <section>
            <h2>Compatibility</h2>
            <dl>
              <dt>Minecraft</dt><dd>{minecraftRange(page.gameVersions)}</dd>
              {#if page.loaders.length}<dt>Loaders</dt><dd>{page.loaders.map(loaderName).join(', ')}</dd>{/if}
              {#if environmentText}<dt>Runs on</dt><dd>{environmentText}</dd>{/if}
            </dl>
          </section>

          {#if fit?.requires.length}
            <section>
              <h2>Also installs</h2>
              {#each fit.requires as need (need.id)}
                <button class="person" onclick={() => openNeeded(need.source, need.id, need.title, need.icon)}>
                  <span class="small-icon">{#if need.icon}<img src={need.icon} alt="" loading="lazy" />{:else}{need.title.charAt(0).toUpperCase()}{/if}</span>
                  <span class="truncate">{need.title}</span>
                </button>
              {/each}
            </section>
          {/if}

          {#if page.links.length}
            <section>
              <h2>Links</h2>
              {#each page.links as link (link.label + link.url)}
                <button class="link" title={link.url} onclick={() => openExternal(link.url)}>{link.label} <ExternalLink size={12} /></button>
              {/each}
            </section>
          {/if}

          {#if page.creators.length}
            <section>
              <h2>Creators</h2>
              {#each page.creators as creator (creator.name)}
                <button class="person" title={creator.url ?? undefined} disabled={!creator.url} onclick={() => creator.url && openExternal(creator.url)}>
                  <span class="small-icon">{#if creator.avatar}<img src={creator.avatar} alt="" loading="lazy" referrerpolicy="no-referrer" />{:else}{creator.name.charAt(0).toUpperCase()}{/if}</span>
                  <span class="who"><span class="truncate">{creator.name}</span>{#if creator.role}<span class="faint small truncate">{creator.role}</span>{/if}</span>
                </button>
              {/each}
            </section>
          {/if}

          <section>
            <h2>Details</h2>
            <dl>
              {#if page.categories.length}<dt>Categories</dt><dd>{page.categories.map(titleCase).join(', ')}</dd>{/if}
              {#if page.license}
                <dt>License</dt>
                <dd>
                  {#if page.license.url}
                    <button class="plain-link" title={page.license.url} onclick={() => openExternal(page!.license!.url)}>{page.license.label}</button>
                  {:else}
                    {page.license.label}
                  {/if}
                </dd>
              {/if}
              {#if page.published}<dt>Published</dt><dd>{day(page.published)}</dd>{/if}
              {#if page.updated}<dt>Updated</dt><dd>{day(page.updated)}</dd>{/if}
            </dl>
          </section>
        </aside>
      {/if}
    </div>
  {:else}
    <p class="message muted">Loading</p>
  {/if}
</div>

{#if viewing}<Lightbox shots={viewing.shots} start={viewing.start} onclose={() => (viewing = null)} />{/if}

<style>
  .project { --side: clamp(256px, 24vw, 330px); height: 100%; overflow-y: auto; scroll-padding-top: 56px; }

  .cover { height: clamp(110px, 20vh, 170px); }
  .cover img { display: block; width: 100%; height: 100%; object-fit: cover; object-position: center 35%; }
  .head {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: 84px minmax(0, 1fr) var(--side);
    gap: 16px;
    align-items: end;
    margin-top: -44px;
    padding: 0 var(--gutter) 14px;
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
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .about { min-width: 0; }
  h1 { font-size: 30px; letter-spacing: -0.025em; overflow-wrap: anywhere; }
  .summary { margin-top: 3px; font-size: var(--text-m); }
  .stats { display: flex; flex-wrap: wrap; margin-top: 5px; }
  .stats span { white-space: nowrap; }
  .stats span + span::before { content: ''; display: inline-block; width: 4px; height: 4px; margin: 0 8px 2px; background: var(--text-3); }

  .actions { display: flex; flex-direction: column; align-items: flex-end; gap: 6px; }
  .target { display: flex; align-items: center; gap: 8px; width: 100%; }
  .target .field { flex: 1; min-width: 0; }
  .fit { min-height: 17px; text-align: right; }
  .have { color: var(--good); font-weight: 600; }
  .site { display: inline-flex; align-items: center; gap: 4px; color: var(--text-2); font-size: var(--text-xs); }
  .site:hover { color: var(--text); }

  .bar { position: sticky; top: 0; z-index: 2; padding: 0 var(--gutter); background: var(--bg); }

  .columns { display: grid; grid-template-columns: minmax(0, 1fr) var(--side); gap: 32px; padding: 16px var(--gutter) 40px; }
  .columns.wide { grid-template-columns: minmax(0, 1fr); }
  .main { min-width: 0; }
  .note { margin-bottom: 16px; padding: 10px 14px; background: var(--panel); clip-path: var(--stepped); color: var(--text-2); }
  .plain { font-size: var(--text-m); line-height: 1.62; white-space: pre-line; }

  aside { min-width: 0; }
  section { padding: 14px 0; border-top: 1px solid var(--line); }
  section:first-child { padding-top: 0; border-top: 0; }
  h2 { margin-bottom: 8px; font-size: var(--text-m); }
  dl { display: grid; grid-template-columns: 78px minmax(0, 1fr); gap: 5px 10px; }
  dt { color: var(--text-3); }
  dd { color: var(--text-2); overflow-wrap: anywhere; }

  .link { display: flex; align-items: center; gap: 5px; padding: 3px 0; color: var(--text-2); }
  .link:hover, .plain-link:hover { color: var(--text); }
  .plain-link { color: inherit; text-align: left; text-decoration: underline; text-underline-offset: 2px; }
  .person { display: flex; align-items: center; gap: 10px; width: 100%; padding: 4px 0; text-align: left; }
  .person:disabled { cursor: default; }
  .person:not(:disabled):hover .truncate:first-child { color: var(--text); }
  .small-icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    overflow: hidden;
    background: var(--control);
    clip-path: var(--stepped);
    color: var(--text-3);
    font-weight: 600;
  }
  .small-icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .who { display: flex; flex-direction: column; min-width: 0; line-height: 1.25; }

  .gallery { display: grid; grid-template-columns: repeat(auto-fill, minmax(250px, 1fr)); gap: 18px 14px; }
  .gallery li { min-width: 0; }
  .gallery button { display: block; width: 100%; aspect-ratio: 16 / 9; overflow: hidden; background: var(--panel); clip-path: var(--stepped); cursor: zoom-in; }
  .gallery img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .gallery p:first-of-type { margin-top: 6px; font-weight: 600; }
  .caption { display: -webkit-box; overflow: hidden; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; }

  .message { padding: 20px var(--gutter); }
  .message .btn { margin-top: 10px; }
</style>

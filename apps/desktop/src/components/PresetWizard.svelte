<script lang="ts">
  import { ChevronLeft, Compass, Gauge, Hammer, Sparkles, Swords, Timer, Wrench } from '@lucide/svelte';
  import { api } from '../lib/api';
  import { explain } from '../lib/errors';
  import { CATEGORIES, presetContents, presetItems } from '../lib/presets';
  import type { Category, Preset } from '../lib/presets';
  import { app } from '../lib/state.svelte';

  /** Asks what the player wants to play, then sets that up as a new instance. */
  let { onclose }: { onclose: () => void } = $props();

  const ICONS = { performance: Gauge, speedrunning: Timer, pvp: Swords, shaders: Sparkles, quality: Wrench, building: Hammer, adventure: Compass } as const;

  let category = $state<Category | null>(null);
  let preset = $state<Preset | null>(null);
  let versions = $state<string[]>([]);
  let finding = $state(false);
  let problem = $state('');
  let minecraft = $state('');
  let name = $state('');

  /** The preset's name, numbered if the library already has one. */
  function unused(base: string): string {
    const taken = new Set(app.library.map((entry) => entry.name));
    let candidate = base;
    for (let number = 2; taken.has(candidate); number++) candidate = `${base} (${number})`;
    return candidate;
  }

  async function choose(chosen: Preset) {
    if (chosen.modpack) return installPack(chosen.modpack);
    preset = chosen;
    name = unused(chosen.title);
    problem = '';
    if (chosen.minecraft) {
      versions = [chosen.minecraft];
      minecraft = chosen.minecraft;
      return;
    }
    // Only versions every core project has a build for, so nothing that matters is left out.
    versions = [];
    minecraft = '';
    finding = true;
    try {
      versions = await api.presetVersions(chosen.loader ?? 'fabric', (chosen.core ?? []).map((item) => item.project));
      minecraft = versions[0] ?? '';
      if (!versions.length) problem = 'No Minecraft version has everything this needs yet.';
    } catch (error) {
      const explained = explain(error);
      problem = explained.sentence || explained.technical;
    }
    finding = false;
  }

  async function create(event: SubmitEvent) {
    event.preventDefault();
    if (!preset || !minecraft) return;
    const request = { name: name.trim() || preset.title, minecraft, loader: preset.loader ?? 'fabric', items: presetItems(preset), speedrun: preset.speedrun ?? null };
    onclose();
    try {
      const result = await api.installPreset(request);
      await app.refresh();
      app.open(result.instance.id);
      const missing = result.skipped.map((item) => item.title);
      app.toast('good', `Set up ${result.instance.name}`, missing.length ? `Not added, with nothing for this version yet: ${missing.join(', ')}` : undefined);
    } catch (error) {
      app.fail(error, 'Could not set that up');
    }
  }

  async function installPack(project: string) {
    onclose();
    try {
      await app.packInstalled(await api.installPack('modrinth', project));
    } catch (error) {
      app.fail(error, 'Could not install that modpack');
    }
  }
</script>

{#if !category}
  <p class="lead">What do you want to play?</p>
  <ul class="categories">
    {#each CATEGORIES as item (item.id)}
      {@const Icon = ICONS[item.id as keyof typeof ICONS]}
      <li>
        <button class="card" onclick={() => (category = item)}>
          <span class="icon"><Icon size={20} /></span>
          <span class="text">
            <span class="title">{item.title}</span>
            <span class="summary">{item.summary}</span>
          </span>
        </button>
      </li>
    {/each}
  </ul>
{:else if !preset}
  <button class="back" onclick={() => (category = null)}><ChevronLeft size={16} /> All categories</button>
  <h3>{category.title}</h3>
  <ul class="options">
    {#each category.presets as item (item.id)}
      <li>
        <button class="card option" onclick={() => choose(item)}>
          <span class="title">{item.title}</span>
          <span class="summary">{item.summary}</span>
          {#if item.note}<span class="note">{item.note}</span>{/if}
          <span class="contents">
            {#if item.modpack}<span>Modpack from Modrinth</span>{:else}{#each presetContents(item) as part (part)}<span>{part}</span>{/each}{/if}
          </span>
        </button>
      </li>
    {/each}
  </ul>
{:else}
  <button class="back" onclick={() => (preset = null)}><ChevronLeft size={16} /> {category.title}</button>
  <form onsubmit={create}>
    <h3>{preset.title}</h3>
    <div class="form-row">
      <label for="preset-name">Name</label>
      <input id="preset-name" class="field" bind:value={name} />
    </div>
    <div class="form-row">
      <label for="preset-version">Minecraft version</label>
      <div>
        {#if finding}
          <p class="fixed faint">Finding versions</p>
        {:else if versions.length > 1}
          <select id="preset-version" class="field" bind:value={minecraft}>
            {#each versions as version (version)}<option value={version}>{version}</option>{/each}
          </select>
        {:else if versions.length === 1}
          <p class="fixed">{versions[0]}</p>
        {:else}
          <p class="fixed failed">{problem}</p>
        {/if}
        <p class="form-hint">{preset.minecraft ? 'This setup is made for this version.' : 'Versions with a build of every mod this setup needs.'}</p>
      </div>
    </div>
    {#if preset.note}<p class="note">{preset.note}</p>{/if}
    <div class="actions">
      <span class="spacer"></span>
      <button class="btn primary" disabled={!minecraft}>Set up</button>
    </div>
  </form>
{/if}

<style>
  .lead { margin-bottom: 12px; color: var(--text-2); }
  .categories { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .options { display: flex; flex-direction: column; gap: 8px; }
  .card { display: flex; gap: 12px; width: 100%; height: 100%; padding: 12px 14px; background: var(--control); clip-path: var(--stepped); text-align: left; transition: background-color 100ms ease-out; }
  .card:hover { background: var(--control-hover); }
  .option { flex-direction: column; gap: 3px; }
  .icon { flex: none; display: grid; place-items: center; width: 36px; height: 36px; background: var(--raised); clip-path: var(--stepped); color: var(--accent-text); }
  .text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .title { font-weight: 600; }
  .summary { color: var(--text-2); font-size: var(--text-xs); }
  .note { color: var(--warn); font-size: var(--text-xs); }
  .contents { display: flex; flex-wrap: wrap; gap: 2px 0; margin-top: 4px; color: var(--text-3); font-size: var(--text-xs); }
  .contents span + span::before { content: ''; display: inline-block; width: 3px; height: 3px; margin: 0 7px 2px; background: var(--text-3); }
  .back { display: flex; align-items: center; gap: 4px; margin-bottom: 8px; color: var(--text-2); }
  .back:hover { color: var(--text); }
  h3 { margin-bottom: 8px; font-size: var(--text-l); }
  .fixed { padding-top: 8px; }
  .failed { color: var(--bad); }
  form .note { margin: 4px 0 0; }
  .actions { display: flex; padding-top: 14px; }
  .spacer { flex: 1; }
</style>

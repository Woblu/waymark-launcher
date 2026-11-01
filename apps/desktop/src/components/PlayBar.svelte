<script lang="ts">
  import { ChevronsUpDown, Plus, Square } from '@lucide/svelte';
  import { instanceIcon } from '../lib/api';
  import { loaderName } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import Art from './Art.svelte';
  import PlayButton from './PlayButton.svelte';

  const task = $derived(app.tasks[0]);
  const percent = $derived(task && task.total > 1 ? Math.min(100, (task.done / task.total) * 100) : null);
  const focused = $derived(app.focused);
  const picture = $derived(focused ? instanceIcon(focused, app.iconTick) : undefined);
  /** Whatever else is running, since the button only speaks for the instance in the bar. */
  const others = $derived(app.library.filter((entry) => entry.running && entry.id !== focused?.id));

  const playing = $derived(app.library.some((entry) => entry.running));

  function choose(event: MouseEvent) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    app.menu = {
      x: box.left,
      y: box.top - 6,
      up: true,
      items: app.library.map((entry) => ({ label: entry.name, run: () => app.open(entry.id) })),
    };
  }
</script>

<footer>
  {#if focused}
    <button class="now" title="Open {focused.name}" onclick={() => app.open(focused.id, app.tab)}>
      <span class="thumb">{#if picture}<img src={picture} alt="" />{:else}<Art seed={focused.name} width={48} height={48} />{/if}</span>
      <span class="text">
        <span class="name truncate">{focused.name}</span>
        <span class="sub truncate">
          {#if focused.running}<span class="running">Running</span>{:else}{loaderName(focused.loader.kind)} {focused.minecraft}{/if}
        </span>
      </span>
    </button>
    {#if app.library.length > 1}
      <button class="swap" aria-label="Choose an instance" title="Choose an instance" onclick={choose}><ChevronsUpDown size={17} /></button>
    {/if}
  {/if}

  <div class="middle">
    {#if task}
      <div class="task" aria-live="polite">
        <span class="truncate">{task.title}: {task.detail}{percent === null ? '' : ` (${percent.toFixed(0)}%)`}{app.tasks.length > 1 ? ` and ${app.tasks.length - 1} more` : ''}</span>
        <span class="bar" class:unknown={percent === null}><span style:width="{percent ?? 100}%"></span></span>
      </div>
    {/if}
    {#each others as entry (entry.id)}
      <div class="other">
        <button class="truncate" title="Open {entry.name}" onclick={() => app.open(entry.id)}><span class="name truncate">{entry.name}</span></button>
        <button class="stop" aria-label="Stop {entry.name}" title="Stop" onclick={() => app.stop(entry.id)}><Square size={11} fill="currentColor" /></button>
      </div>
    {/each}
    {#if app.update}
      <button class="btn" disabled={playing} title={playing ? 'After your game closes' : `Installs Waymark ${app.update}`} onclick={() => app.installUpdate()}>Restart to update</button>
    {/if}
  </div>

  {#if focused}
    <PlayButton entry={focused} />
  {:else if app.loaded}
    <button class="btn primary start" onclick={() => (app.dialog = 'new')}><Plus size={17} /> New instance</button>
  {/if}
</footer>

<style>
  footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--playbar);
    padding: 0 var(--gutter);
    background: var(--panel);
  }
  .now { display: flex; align-items: center; gap: 12px; min-width: 0; max-width: 300px; text-align: left; }
  .thumb { flex: none; display: block; overflow: hidden; width: 46px; height: 46px; background: var(--raised); clip-path: var(--stepped); }
  .thumb :global(img) { display: block; width: 100%; height: 100%; object-fit: cover; }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .name { font-size: var(--text-m); font-weight: 650; }
  .now:hover .name { text-decoration: underline; text-underline-offset: 2px; }
  .sub { color: var(--text-3); font-size: var(--text-xs); }
  .running { color: var(--good); font-weight: 600; }
  .swap { flex: none; display: grid; place-items: center; width: 30px; height: 36px; color: var(--text-3); }
  .swap:hover { color: var(--text); }

  .middle { flex: 1; display: flex; align-items: center; justify-content: flex-end; gap: 12px; min-width: 0; padding: 0 12px; }
  .task { flex: 0 1 380px; display: flex; flex-direction: column; gap: 5px; min-width: 0; color: var(--text-2); font-size: var(--text-xs); }
  .bar { display: block; overflow: hidden; width: 100%; height: 4px; background: var(--control); }
  .bar span { display: block; height: 100%; background: var(--accent); }
  .bar.unknown span { opacity: 0.45; }
  .other { flex: none; display: flex; align-items: center; max-width: 180px; color: var(--text-2); }
  .other > button:first-child { min-width: 0; }
  .other .name { font-size: var(--text-s); font-weight: 600; }
  .stop { flex: none; display: grid; place-items: center; width: 28px; height: 28px; color: var(--text-3); }
  .stop:hover { color: var(--bad); }
  .start { height: 44px; padding: 0 18px; font-size: var(--text-m); }
</style>

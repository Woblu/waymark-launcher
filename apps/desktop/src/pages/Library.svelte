<script lang="ts">
  import { Plus } from '@lucide/svelte';
  import Art from '../components/Art.svelte';
  import { loaderName } from '../lib/format';
  import { app } from '../lib/state.svelte';
  import Instance from './Instance.svelte';

  let filter = $state('');

  const shown = $derived.by(() => {
    const text = filter.trim().toLowerCase();
    return text ? app.library.filter((entry) => entry.name.toLowerCase().includes(text)) : app.library;
  });
  const current = $derived(app.current);
</script>

<div class="library">
  <aside aria-label="Instances">
    <div class="head">
      <input class="field" placeholder="Filter instances" aria-label="Filter instances" bind:value={filter} />
      <button class="btn icon" title="Add an instance" aria-label="Add an instance" onclick={() => (app.dialog = 'new')}><Plus size={15} /></button>
    </div>
    <ul>
      {#each shown as entry (entry.id)}
        <li>
          <button class="row" class:selected={current?.id === entry.id} aria-current={current?.id === entry.id} onclick={() => app.open(entry.id, app.tab)}>
            <span class="thumb">
              {#if entry.icon}<img src={entry.icon} alt="" />{:else}<Art seed={entry.name} width={36} height={36} />{/if}
            </span>
            <span class="text">
              <span class="name truncate">{entry.name}</span>
              <span class="sub truncate">
                {#if entry.running}<span class="running">Running</span>{:else}{entry.minecraft} {loaderName(entry.loader.kind)}{/if}
              </span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  </aside>

  <div class="detail">
    {#if current}
      {#key current.id}
        <Instance id={current.id} />
      {/key}
    {:else if app.loaded}
      <div class="banner"><Art seed="Waymark" /></div>
      <div class="welcome">
        <h1>No instances yet</h1>
        <p class="muted">Create an instance, or import one from Prism Launcher, MultiMC, CurseForge or the Modrinth App. Importing leaves the original alone.</p>
        <div class="actions">
          <button class="btn primary" onclick={() => (app.dialog = 'new')}>Add an instance</button>
          <button class="btn" onclick={() => app.go('discover')}>Browse modpacks</button>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .library { display: grid; grid-template-columns: var(--sidebar) minmax(0, 1fr); height: 100%; }
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--panel);
    border-right: 1px solid var(--line);
  }
  .head { flex: none; display: flex; gap: 6px; padding: 10px; }
  ul { flex: 1; min-height: 0; overflow-y: auto; padding: 0 6px 6px; }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px;
    border-radius: var(--radius);
    text-align: left;
  }
  .row:hover { background: var(--raised); }
  .row.selected { background: var(--selected); }
  .thumb { flex: none; width: 36px; height: 36px; overflow: hidden; }
  .thumb img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .name { font-weight: 500; }
  .sub { color: var(--text-3); font-size: var(--text-xs); }
  .running { color: var(--good); }
  .detail { min-width: 0; min-height: 0; }
  .banner { height: 176px; }
  .welcome { padding: 18px 24px; max-width: 560px; }
  h1 { font-size: var(--text-xl); letter-spacing: -0.015em; }
  .welcome p { margin-top: 6px; }
  .actions { display: flex; gap: 8px; margin-top: 14px; }
</style>

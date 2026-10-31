<script lang="ts">
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  const crash = $derived(app.crash!);
  const entry = $derived(app.entry(crash.instance));
  const unclear = $derived(crash.report.findings.every((finding) => finding.fix.kind === 'runBisect'));

  function close() {
    app.crash = null;
  }

  async function search() {
    const id = crash.instance;
    close();
    try {
      await api.startBisect(id);
      await app.refresh();
      app.contentTick++;
      app.open(id, 'mods');
    } catch (error) {
      app.fail(error, 'Could not start the search');
    }
  }

  function openTab(tab: 'trail' | 'settings') {
    const id = crash.instance;
    close();
    app.open(id, tab);
  }
</script>

<Modal title="{entry?.name ?? 'The game'} crashed" width={520} onclose={close}>
  <ul>
    {#each crash.report.findings as finding, index (index)}
      <li>
        <strong>{finding.title}</strong>
        <p class="muted">{finding.detail}</p>
        {#if finding.fix.kind === 'raiseMemory'}
          <button class="btn" onclick={() => openTab('settings')}>Change memory</button>
        {/if}
      </li>
    {/each}
  </ul>
  <div class="actions">
    <button class="btn" class:primary={!unclear} onclick={() => openTab('trail')}>Go back to a waymark</button>
    <button class="btn" class:primary={unclear} onclick={search}>Search for the cause</button>
  </div>
</Modal>

<style>
  li { padding: 10px 0; border-bottom: 1px solid var(--line); }
  li:first-child { padding-top: 0; }
  strong { font-size: var(--text-m); font-weight: 600; }
  p { margin-top: 2px; }
  li .btn { margin-top: 8px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
</style>

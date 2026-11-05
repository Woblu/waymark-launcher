<script lang="ts">
  import { api } from '../lib/api';
  import type { Finding } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  const crash = $derived(app.crash!);
  const entry = $derived(app.entry(crash.instance));
  const unclear = $derived(crash.report.findings.every((finding) => finding.fix.kind === 'runBisect'));
  /** The crash report as the Logs tab lists it, like `crash-reports/crash-2026-11-04.txt`. */
  const reportPath = $derived(crash.report.reportFile ? `crash-reports/${crash.report.reportFile.split(/[\\/]/).pop()}` : null);

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

  function openTab(tab: 'trail' | 'settings' | 'mods', field?: string) {
    const id = crash.instance;
    close();
    app.focusField = field ?? null;
    app.open(id, tab);
  }

  /** Opens the Mods tab already searching for the mod a finding named. */
  function findInMods(text: string) {
    const id = crash.instance;
    close();
    if (text && entry?.loader.kind !== 'vanilla') app.prefill = { where: 'mods', text };
    app.open(id, 'mods');
  }

  /** Discover, searching for the missing mod, ready to install into this instance. */
  function findInDiscover(text: string) {
    const id = crash.instance;
    close();
    app.go('discover', id, 'mod');
    app.prefill = { where: 'discover', text };
  }

  /** The crash report, or else this session's log, or latest.log if the window was closed while it ran. */
  function openLog(path: string | null, share = false) {
    const id = crash.instance;
    close();
    const live = (app.logs.get(id)?.length ?? 0) > 0;
    app.logPick = { instance: id, path: path ?? (live ? '' : 'logs/latest.log'), share };
    app.open(id, 'logs');
  }

  function named(finding: Finding): string {
    return 'id' in finding.fix ? finding.fix.id : (finding.suspects[0] ?? '');
  }
</script>

<Modal title="{entry?.name ?? 'The game'} crashed" width={540} onclose={close}>
  <ul>
    {#each crash.report.findings as finding, index (index)}
      <li>
        <strong>{finding.title}</strong>
        <p class="muted">{finding.detail}</p>
        {#if finding.fix.kind === 'raiseMemory'}
          <div class="fix"><button class="btn" onclick={() => openTab('settings', 'memory')}>Change memory</button></div>
        {:else if finding.fix.kind === 'useJava'}
          <p class="faint small how">{`Install Java ${finding.fix.major} or newer, then choose it as the Java path in this instance's settings.`}</p>
          <div class="fix"><button class="btn" onclick={() => openTab('settings', 'java')}>Open Java settings</button></div>
        {:else if finding.fix.kind === 'installMod'}
          {@const missing = named(finding)}
          <div class="fix">
            <button class="btn" onclick={() => findInDiscover(missing)}>Find it in Discover</button>
            <button class="btn quiet" onclick={() => findInMods('')}>Open Mods</button>
          </div>
        {:else if finding.fix.kind === 'removeDuplicate'}
          {@const duplicate = named(finding)}
          <p class="faint small how">Turn off or remove one of the two copies in the Mods tab.</p>
          <div class="fix"><button class="btn" onclick={() => findInMods(duplicate)}>Open Mods</button></div>
        {:else if finding.fix.kind === 'updateDrivers'}
          <p class="faint small how">Get the newest driver from your graphics card maker's website, NVIDIA, AMD or Intel, then start the game again.</p>
        {/if}
      </li>
    {/each}
  </ul>
  <div class="files">
    {#if reportPath}<button class="link" onclick={() => openLog(reportPath)}>Open crash report</button>{/if}
    <button class="link" onclick={() => openLog(null)}>Open log</button>
    <button class="link" onclick={() => openLog(reportPath, true)}>{reportPath ? 'Share crash report' : 'Share log'}</button>
  </div>
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
  .how { margin-top: 6px; }
  .fix { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 8px; }
  .files { display: flex; flex-wrap: wrap; gap: 4px 16px; margin-top: 12px; }
  .link { color: var(--text-2); font-weight: 600; text-decoration: underline; text-underline-offset: 2px; }
  .link:hover { color: var(--text); }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
</style>

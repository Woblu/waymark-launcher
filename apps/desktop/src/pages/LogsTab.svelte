<script lang="ts">
  import { untrack } from 'svelte';
  import { Copy, Search, Share2 } from '@lucide/svelte';
  import VirtualList from '../components/VirtualList.svelte';
  import Modal from '../components/Modal.svelte';
  import { api, openExternal } from '../lib/api';
  import type { LogFile } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { LogLine } from '../lib/state.svelte';

  let { id }: { id: string } = $props();

  const LIVE = '';

  let query = $state('');
  let errorsOnly = $state(false);
  let files = $state<LogFile[]>([]);
  let chosen = $state(LIVE);
  let saved = $state<LogLine[]>([]);
  /** Which file `saved` holds, so a log still loading can't be shared half read. */
  let savedPath = $state<string | null>(null);
  let sharing = $state(false);
  let uploading = $state(false);

  // A log asked for from elsewhere, like the crash dialog's Open crash report,
  // whether this tab opens for it or was already open.
  $effect(() => {
    const pick = app.logPick;
    if (pick?.instance !== id) return;
    untrack(() => {
      app.logPick = null;
      chosen = pick.path;
      sharing = !!pick.share;
    });
  });

  const running = $derived(app.entry(id)?.running ?? false);

  // Listed when the tab opens and again when the game starts or stops, which is
  // when files appear, rather than with every batch of lines.
  $effect(() => {
    void running;
    api.logFiles(id).then((found) => (files = found), () => {});
  });

  $effect(() => {
    const path = chosen;
    if (path === LIVE) return;
    let current = true;
    api.readLog(id, path).then(
      (text) => {
        if (!current) return;
        saved = text.split('\n').map((line) => ({ text: line, error: /\/(ERROR|FATAL)\]|^Caused by|Exception/.test(line) }));
        savedPath = path;
      },
      (error) => app.fail(error, 'Could not read that log'),
    );
    return () => (current = false);
  });

  /**
   * The live log grows in place, so with nothing filtered it's shown as it is,
   * and logTick tells the list it has more lines. Only a filter makes a copy.
   */
  const lines = $derived.by(() => {
    void app.logTick;
    const all = chosen === LIVE ? (app.logs.get(id) ?? []) : saved;
    const text = query.trim().toLowerCase();
    if (!text && !errorsOnly) return all;
    return all.filter((line) => (!errorsOnly || line.error) && (!text || line.text.toLowerCase().includes(text)));
  });
  const count = $derived.by(() => {
    void app.logTick;
    return lines.length;
  });

  function tone(line: LogLine): string {
    if (line.error) return 'error';
    return line.text.includes('/WARN]') ? 'warn' : '';
  }

  async function copyAll() {
    await navigator.clipboard.writeText(lines.map((l) => l.text).join('\n'));
    app.toast('good', `Copied ${lines.length} lines`);
  }

  async function upload() {
    uploading = true;
    try {
      const all = chosen === LIVE ? (app.logs.get(id) ?? []) : saved;
      const url = await api.uploadLog(all.map((line) => line.text).join('\n'));
      await navigator.clipboard.writeText(url);
      sharing = false;
      app.toast('good', 'Log shared, and the link is copied', url, { label: 'Open', run: () => openExternal(url) });
    } catch (error) {
      app.fail(error, 'Could not share that log');
    }
    uploading = false;
  }
</script>

<div class="tab">
  <div class="tools">
    <select class="field which" aria-label="Which log" bind:value={chosen}>
      <option value={LIVE}>This session</option>
      {#if chosen !== LIVE && !files.some((file) => file.path === chosen)}<option value={chosen}>{chosen.split('/').pop()}</option>{/if}
      {#each files as file (file.path)}<option value={file.path}>{file.path.split('/').pop()}</option>{/each}
    </select>
    <label class="search-box search"><Search size={16} /><input class="field" placeholder="Search the log" aria-label="Search the log" bind:value={query} /></label>
    <label class="check"><input type="checkbox" bind:checked={errorsOnly} /> Errors only</label>
    <span class="spacer"></span>
    <button class="btn" disabled={!count} onclick={copyAll}><Copy size={15} /> Copy</button>
    <button class="btn" disabled={!count} onclick={() => (sharing = true)}><Share2 size={15} /> Share</button>
  </div>

  <div class="console">
    {#if count}
      <VirtualList items={lines} rowHeight={20} tick={app.logTick} stickToEnd>
        {#snippet row(line: LogLine)}
          <div class="line mono selectable {tone(line)}">{line.text}</div>
        {/snippet}
      </VirtualList>
    {:else}
      <p class="faint">Output from the game appears here while it runs.</p>
    {/if}
  </div>
</div>

{#if sharing}
  <Modal title="Share this log" width={440} onclose={() => (sharing = false)}>
    <p class="muted">This uploads the log to mclo.gs, and anyone with the link can read it. Waymark Launcher removes your folder names and account tokens first, but other details from your game can still be in there.</p>
    <div class="buttons">
      <button class="btn" onclick={() => (sharing = false)}>Cancel</button>
      <button class="btn primary" disabled={uploading || (chosen !== LIVE && savedPath !== chosen)} onclick={upload}>{uploading ? 'Uploading' : 'Upload'}</button>
    </div>
  </Modal>
{/if}

<style>
  .tab { display: flex; flex-direction: column; height: 100%; }
  .tools { flex: none; display: flex; align-items: center; gap: 10px; min-height: 64px; padding: 14px var(--gutter); }
  .which { width: 200px; }
  .search { flex: 0 1 240px; }
  .check { display: flex; align-items: center; gap: 7px; margin-left: 4px; color: var(--text-2); }
  .spacer { flex: 1; }
  .console { flex: 1; min-height: 0; margin: 0 var(--gutter) var(--gutter); padding: 10px 4px 10px 14px; background: var(--console); clip-path: var(--stepped); }
  .line { overflow: hidden; color: var(--text-2); font-size: 12px; line-height: 20px; text-overflow: ellipsis; white-space: pre; }
  .warn { color: var(--warn); }
  .error { color: var(--bad); }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; padding-top: 16px; }
</style>

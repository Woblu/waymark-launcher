<script lang="ts">
  import { onMount } from 'svelte';
  import { ExternalLink } from '@lucide/svelte';
  import { api, openExternal } from '../lib/api';
  import type { ManualFile } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  let { id, files, onchange, onclose }: { id: string; files: ManualFile[]; onchange: (left: ManualFile[]) => void; onclose: () => void } = $props();

  const CHECK_EVERY = 2000;

  onMount(() => {
    const timer = setInterval(async () => {
      const left = await api.collectManualFiles(id).catch(() => null);
      if (!left || left.length === files.length) return;
      onchange(left);
      app.contentTick++;
      if (!left.length) onclose();
    }, CHECK_EVERY);
    return () => clearInterval(timer);
  });
</script>

<Modal title="Files to download" width={520} {onclose}>
  <p class="muted intro">
    The authors of these mods only allow downloads from CurseForge. Open each page and download the file. Waymark watches your Downloads folder and installs them as they arrive.
  </p>
  <ul>
    {#each files as file (file.file)}
      <li>
        <div class="text">
          <span class="truncate">{file.name}</span>
          <span class="faint small truncate selectable">{file.file}</span>
        </div>
        <button class="btn" onclick={() => openExternal(file.page)}>Open page <ExternalLink size={12} /></button>
      </li>
    {/each}
  </ul>
  <div class="buttons">
    <span class="faint small">Watching for {files.length} {files.length === 1 ? 'file' : 'files'}</span>
    <button class="btn" onclick={onclose}>Close</button>
  </div>
</Modal>

<style>
  .intro { margin-bottom: 10px; }
  ul { max-height: 50vh; overflow-y: auto; border-top: 1px solid var(--line); }
  li { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 7px 0; border-bottom: 1px solid var(--line); }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .buttons { display: flex; align-items: center; justify-content: space-between; padding-top: 12px; }
</style>

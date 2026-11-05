<script lang="ts">
  import type { PublicServer } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  let { server, onclose }: { server: PublicServer; onclose: () => void } = $props();

  const pack = $derived(server.content.kind === 'modpack' ? (server.content.name ?? server.name) : null);

  function join() {
    onclose();
    app.joinServer(server);
  }
</script>

<Modal title={`Join ${server.name}`} width={460} {onclose}>
  <div class="what">
    <span class="icon">{#if server.icon}<img src={server.icon} alt="" />{:else}{server.name.charAt(0).toUpperCase()}{/if}</span>
    <div class="about">
      <p class="name truncate">{server.name}</p>
      <p class="faint mono truncate">{server.address}</p>
    </div>
  </div>
  {#if pack}
    <p class="muted">{`This server plays with the ${pack} modpack. Waymark Launcher installs the exact version it uses, then joins as soon as it's ready.`}</p>
  {/if}
  <p class="muted">The new instance stays linked to this server, so joining again starts it straight away, and it follows the server when it moves to a new version.</p>
  <div class="actions">
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={join}>Install and join</button>
  </div>
</Modal>

<style>
  .what { display: flex; align-items: center; gap: 12px; margin-bottom: 14px; }
  .icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    overflow: hidden;
    background: var(--control);
    clip-path: var(--stepped);
    color: var(--text-3);
    font-weight: 600;
  }
  .icon img { display: block; width: 100%; height: 100%; object-fit: cover; }
  .about { min-width: 0; }
  .name { font-weight: 600; }
  p + p { margin-top: 8px; }
  .actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 16px; padding-top: 12px; border-top: 1px solid var(--line); }
</style>

<script lang="ts">
  import { ExternalLink } from '@lucide/svelte';
  import VirtualList from '../components/VirtualList.svelte';
  import { api, openExternal, openFolder } from '../lib/api';
  import type { HostStatus } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import type { LogLine } from '../lib/state.svelte';

  let { id }: { id: string } = $props();

  const EULA = 'https://aka.ms/MinecraftEULA';

  let status = $state<HostStatus | null>(null);
  let busy = $state(false);
  let accepted = $state(false);
  let command = $state('');

  const key = $derived(`server:${id}`);
  const lines = $derived.by((): LogLine[] => {
    app.logTick;
    return (app.logs.get(key) ?? []).slice();
  });

  async function load() {
    status = await api.hostStatus(id).catch(() => null);
  }

  $effect(() => {
    app.contentTick;
    load();
  });

  async function prepare() {
    busy = true;
    try {
      status = await api.prepareHost(id);
      app.toast('good', 'Server ready');
    } catch (error) {
      app.fail(error, 'Could not set up the server');
    }
    busy = false;
  }

  async function start() {
    busy = true;
    try {
      if (!status?.eulaAccepted) await api.acceptEula(id);
      app.logs.set(key, []);
      app.logTick++;
      await api.startHost(id);
      await load();
    } catch (error) {
      app.fail(error, 'Could not start the server');
    }
    busy = false;
  }

  async function stop() {
    try {
      await api.stopHost(id);
    } catch (error) {
      app.fail(error);
    }
  }

  async function send(event: SubmitEvent) {
    event.preventDefault();
    const line = command.trim();
    if (!line) return;
    command = '';
    await api.hostCommand(id, line).catch((error) => app.fail(error));
  }

  async function openServerFolder() {
    await openFolder(await api.hostFolder(id));
  }
</script>

<div class="tab">
  {#if status}
    <div class="tools">
      {#if !status.ready}
        <div class="intro">
          <strong>Host a server from this computer</strong>
          <p class="muted">Friends can join your world while the server is running. Waymark builds it from this instance and leaves out mods that only work on the client.</p>
        </div>
        <button class="btn primary" disabled={busy} onclick={prepare}>{busy ? 'Setting up' : 'Set up a server'}</button>
      {:else}
        <span class="state" class:on={status.running}>{status.running ? 'Running' : 'Stopped'}</span>
        {#if status.address}<span class="muted selectable">{status.address}</span>{/if}
        <span class="spacer"></span>
        {#if !status.running}
          <button class="btn" disabled={busy} title="Copies this instance's current mods to the server" onclick={prepare}>Update mods</button>
          <button class="btn" onclick={openServerFolder}>Open folder</button>
        {/if}
        {#if status.running}
          <button class="btn" onclick={stop}>Stop</button>
        {:else}
          <button class="btn primary" disabled={busy || (!status.eulaAccepted && !accepted)} onclick={start}>Start</button>
        {/if}
      {/if}
    </div>

    {#if status.ready && !status.eulaAccepted}
      <div class="eula">
        <label class="check"><input type="checkbox" bind:checked={accepted} /> I accept the Minecraft End User License Agreement</label>
        <button class="link" onclick={() => openExternal(EULA)}>Read it <ExternalLink size={12} /></button>
      </div>
    {/if}

    {#if status.ready}
      <p class="faint small note">
        People on your network can join at the address above. To let friends join over the internet, forward port {status.port} on your router, and only share that address with people you trust.
      </p>
      <div class="console">
        {#if lines.length}
          <VirtualList items={lines} rowHeight={20} stickToEnd>
            {#snippet row(line: LogLine)}
              <div class="line mono selectable">{line.text}</div>
            {/snippet}
          </VirtualList>
        {:else}
          <p class="faint">Output from the server appears here while it runs.</p>
        {/if}
      </div>
      <form class="command" onsubmit={send}>
        <input class="field mono" placeholder="Type a server command, like list" aria-label="Server command" disabled={!status.running} bind:value={command} />
        <button class="btn" disabled={!status.running || !command.trim()}>Send</button>
      </form>
    {/if}
  {/if}
</div>

<style>
  .tab { display: flex; flex-direction: column; height: 100%; }
  .tools { flex: none; display: flex; align-items: center; gap: 12px; min-height: 64px; padding: 14px var(--gutter); }
  .intro { flex: 1; min-width: 0; }
  strong { font-weight: 600; }
  .state { color: var(--text-3); font-weight: 600; }
  .state.on { color: var(--good); }
  .spacer { flex: 1; }
  .eula { flex: none; display: flex; align-items: center; gap: 14px; margin: 0 var(--gutter) 10px; padding: 10px 14px; background: var(--raised); clip-path: var(--stepped); }
  .check { display: flex; align-items: center; gap: 8px; cursor: pointer; }
  .link { display: inline-flex; align-items: center; gap: 4px; color: var(--text-2); }
  .link:hover { color: var(--text); }
  .note { flex: none; padding: 0 var(--gutter) 10px; }
  .console { flex: 1; min-height: 0; margin: 0 var(--gutter); padding: 10px 4px 10px 14px; background: var(--console); clip-path: var(--stepped); }
  .line { overflow: hidden; color: var(--text-2); font-size: 12px; line-height: 20px; text-overflow: ellipsis; white-space: pre; }
  .command { flex: none; display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 8px; padding: 10px var(--gutter) var(--gutter); }
</style>

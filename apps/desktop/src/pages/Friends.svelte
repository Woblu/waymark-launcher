<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Check, Ellipsis, Plus, Trash2, UserPlus, X } from '@lucide/svelte';
  import Art from '../components/Art.svelte';
  import Modal from '../components/Modal.svelte';
  import OfflineLine from '../components/OfflineLine.svelte';
  import { api } from '../lib/api';
  import { tabKeys } from '../lib/tabs';
  import type { Friend, LibraryEntry, OtherPlayer, SharedByFriend } from '../lib/api';
  import { timeAgo } from '../lib/format';
  import { app } from '../lib/state.svelte';

  const REFRESH_EVERY = 30000;

  let mode = $state<'in' | 'new'>('in');
  let username = $state('');
  let password = $state('');
  let busy = $state(false);
  let adding = $state('');
  let confirming = $state(false);
  let installing = $state<string | null>(null);
  let joining = $state<string | null>(null);
  /** An instance about to be sent that has files no store has, which a share can't carry. */
  let stranded = $state<{ friend: Friend; entry: LibraryEntry; files: string[] } | null>(null);

  const overview = $derived(app.friends);
  const online = $derived(overview?.friends.filter((friend) => friend.online) ?? []);
  const away = $derived(overview?.friends.filter((friend) => !friend.online) ?? []);

  /** Gets their setup if it's new here, then goes straight into their server. */
  async function join(friend: Friend) {
    if (!friend.hosting) return;
    // Asked before their setup downloads, not after.
    if (!app.activeAccount) {
      app.askToSignIn(`Sign in to join ${friend.username}.`);
      return;
    }
    joining = friend.id;
    try {
      const result = await api.friendJoin(friend.id);
      await app.refresh();
      if (result.failed.length) app.toast('info', `${result.failed.length} could not be installed`, result.failed.slice(0, 4).join(', '));
      await app.play(result.instance.id, { kind: 'server', address: friend.hosting.address });
    } catch (error) {
      app.fail(error, `Could not join ${friend.username}`);
    }
    joining = null;
  }

  /** Runs a change on the server, then shows how things stand now. */
  async function change(work: () => Promise<unknown>, failure?: string) {
    try {
      await work();
      await app.loadFriends();
    } catch (error) {
      app.fail(error, failure);
    }
  }

  async function enter(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    try {
      app.friendAccount = await (mode === 'new' ? api.friendSignUp(username, password) : api.friendSignIn(username, password));
      password = '';
      await app.loadFriends();
    } catch (error) {
      app.fail(error, mode === 'new' ? 'Could not make that account' : 'Could not sign in');
    }
    busy = false;
  }

  async function add(event: SubmitEvent) {
    event.preventDefault();
    const name = adding.trim();
    if (!name) return;
    await change(() => api.friendRequest(name), 'Could not send that request');
    adding = '';
  }

  async function signOut() {
    await change(() => api.friendSignOut());
    app.friendAccount = null;
    app.friends = null;
  }

  async function deleteAccount() {
    confirming = false;
    try {
      await api.friendDeleteAccount();
      app.friendAccount = null;
      app.friends = null;
      app.toast('good', 'Account deleted');
    } catch (error) {
      app.fail(error, 'Could not delete your account');
    }
  }

  function send(event: MouseEvent, friend: Friend) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    app.menu = {
      x: box.left,
      y: box.bottom + 4,
      items: app.library.map((entry) => ({ label: entry.name, run: () => sendInstance(friend, entry) })),
    };
  }

  async function sendInstance(friend: Friend, entry: LibraryEntry, anyway = false) {
    if (!anyway) {
      const files = await api.shareCode(entry.id).then((share) => share.leftOut, () => []);
      if (files.length) {
        stranded = { friend, entry, files };
        return;
      }
    }
    stranded = null;
    await change(async () => {
      await api.friendSendInstance(friend.id, entry.id);
      app.toast('good', `Sent ${entry.name} to ${friend.username}`);
    }, 'Could not send that');
  }

  function exportInstead() {
    if (!stranded) return;
    app.instanceDialog = { kind: 'export', id: stranded.entry.id };
    stranded = null;
  }

  function more(event: MouseEvent, friend: Friend) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    app.menu = {
      x: box.left,
      y: box.bottom + 4,
      items: [
        { label: 'Remove friend', run: () => change(() => api.friendRemove(friend.id)) },
        { label: 'Block', danger: true, run: () => change(() => api.friendBlock(friend.id)) },
      ],
    };
  }

  async function install(share: SharedByFriend) {
    installing = share.id;
    try {
      const result = await api.friendInstallShare(share.id);
      await app.refresh();
      await app.loadFriends();
      app.open(result.instance.id);
    } catch (error) {
      app.fail(error, 'Could not install that');
    }
    installing = null;
  }

  let timer: ReturnType<typeof setInterval>;

  onMount(() => {
    app.loadFriends();
    timer = setInterval(() => app.loadFriends(), REFRESH_EVERY);
  });

  onDestroy(() => clearInterval(timer));
</script>

{#snippet person(name: string)}
  <span class="face"><Art seed={name} width={40} height={40} /></span>
{/snippet}

{#snippet row(friend: Friend)}
  <li class="friend">
    {@render person(friend.username)}
    <span class="text">
      <span class="name truncate">{friend.username}</span>
      <span class="sub truncate">
        {#if friend.hosting}<span class="on">Hosting {friend.hosting.title}</span>
        {:else if friend.playing}<span class="on">Playing {friend.playing}</span>
        {:else if friend.online}<span class="on">Online</span>
        {:else if friend.lastSeen}Last seen {timeAgo(friend.lastSeen)}
        {:else}Offline{/if}
      </span>
    </span>
    {#if friend.hosting}<button class="btn primary" disabled={joining !== null} onclick={() => join(friend)}>{joining === friend.id ? 'Getting ready' : 'Join'}</button>{/if}
    {#if app.library.length}<button class="btn" onclick={(event) => send(event, friend)}>Send instance</button>{/if}
    <button class="btn icon quiet" aria-label="More for {friend.username}" title="More" onclick={(event) => more(event, friend)}><Ellipsis size={17} /></button>
  </li>
{/snippet}

{#snippet asked(other: OtherPlayer, incoming: boolean)}
  <li class="friend">
    {@render person(other.username)}
    <span class="text">
      <span class="name truncate">{other.username}</span>
      <span class="sub">{incoming ? 'Wants to be friends' : 'Waiting for them to accept'}</span>
    </span>
    {#if incoming}<button class="btn primary" onclick={() => change(() => api.friendRespond(other.id, true))}><Check size={15} /> Accept</button>{/if}
    <button class="btn icon quiet" aria-label={incoming ? 'Decline' : 'Cancel request'} title={incoming ? 'Decline' : 'Cancel request'} onclick={() => change(() => api.friendRespond(other.id, false))}><X size={17} /></button>
  </li>
{/snippet}

<div class="page">
  <div class="scene top">
    <Art seed="Waymark friends" width={480} height={100} />
    <div class="title">
      <h1>Friends</h1>
      {#if overview}<p class="muted">Signed in as <span class="name">{overview.username}</span></p>{/if}
    </div>
  </div>

  {#if !app.friendsAvailable}
    <div class="blank">
      <h2>Friends aren't available yet</h2>
      <p class="muted">This copy of Waymark Launcher isn't connected to the friends server.</p>
    </div>
  {:else if !app.friendAccount}
    <div class="entry">
      {#if app.offline}<OfflineLine what="Signing in to friends needs a connection." />{/if}
      <div class="tabs" role="tablist" aria-label="Account" use:tabKeys>
        <button role="tab" aria-selected={mode === 'in'} tabindex={mode === 'in' ? 0 : -1} class:active={mode === 'in'} onclick={() => (mode = 'in')}>Sign in</button>
        <button role="tab" aria-selected={mode === 'new'} tabindex={mode === 'new' ? 0 : -1} class:active={mode === 'new'} onclick={() => (mode = 'new')}>Create account</button>
      </div>
      <form onsubmit={enter}>
        <label for="friend-name">Username</label>
        <input id="friend-name" class="field" autocomplete="username" required bind:value={username} />
        <label for="friend-password">Password</label>
        <input id="friend-password" class="field" type="password" autocomplete={mode === 'new' ? 'new-password' : 'current-password'} required minlength={mode === 'new' ? 8 : undefined} bind:value={password} />
        <button class="btn primary" disabled={busy}>{mode === 'new' ? 'Create account' : 'Sign in'}</button>
      </form>
      {#if mode === 'new'}
        <p class="form-hint">Separate from your Minecraft account. There's no email, so a lost password can't be recovered.</p>
      {/if}
    </div>
  {:else if overview}
    <div class="body">
      <div class="people">
        {#if app.offline}<OfflineLine what="Who's online may be out of date until the connection is back." onretry={() => app.loadFriends()} />{/if}
        <form class="adder" onsubmit={add}>
          <input class="field" placeholder="Add a friend by username" aria-label="Add a friend by username" bind:value={adding} />
          <button class="btn primary" disabled={!adding.trim()}><UserPlus size={16} /> Add</button>
        </form>

        {#if overview.incoming.length || overview.outgoing.length}
          <section>
            <h2>Requests</h2>
            <ul>
              {#each overview.incoming as other (other.id)}{@render asked(other, true)}{/each}
              {#each overview.outgoing as other (other.id)}{@render asked(other, false)}{/each}
            </ul>
          </section>
        {/if}

        {#if online.length}
          <section>
            <h2>Online</h2>
            <ul>{#each online as friend (friend.id)}{@render row(friend)}{/each}</ul>
          </section>
        {/if}
        {#if away.length}
          <section>
            <h2>Offline</h2>
            <ul>{#each away as friend (friend.id)}{@render row(friend)}{/each}</ul>
          </section>
        {/if}
        {#if !overview.friends.length && !overview.incoming.length && !overview.outgoing.length}
          <div class="blank">
            <h2>No friends yet</h2>
            <p class="muted">Add friends by their Waymark Launcher username.</p>
          </div>
        {/if}
      </div>

      <aside>
        {#if overview.shares.length}
          <section>
            <h2>Sent to you</h2>
            <ul>
              {#each overview.shares as share (share.id)}
                <li class="share">
                  <span class="text">
                    <span class="name truncate">{share.name}</span>
                    <span class="sub truncate">From {share.from}, {timeAgo(share.at)}</span>
                  </span>
                  <button class="btn" disabled={installing !== null} onclick={() => install(share)}><Plus size={15} /> {installing === share.id ? 'Installing' : 'Install'}</button>
                  <button class="btn icon quiet" aria-label="Dismiss {share.name}" title="Dismiss" onclick={() => change(() => api.friendDropShare(share.id))}><X size={17} /></button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        <section>
          <h2>Your account</h2>
          <label class="check"><input type="checkbox" checked={overview.showStatus} onchange={(event) => { const shown = event.currentTarget.checked; change(() => api.friendShowStatus(shown)); }} /> Show friends when I'm online</label>
          <div class="actions">
            <button class="btn" onclick={signOut}>Sign out</button>
            <button class="btn danger" onclick={() => (confirming = true)}><Trash2 size={15} /> Delete account</button>
          </div>
        </section>

        {#if overview.blocked.length}
          <section>
            <h2>Blocked</h2>
            <ul>
              {#each overview.blocked as other (other.id)}
                <li class="share">
                  <span class="text"><span class="name truncate">{other.username}</span></span>
                  <button class="btn" onclick={() => change(() => api.friendUnblock(other.id))}>Unblock</button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </aside>
    </div>
  {/if}
</div>

{#if confirming}
  <Modal title="Delete your account" width={420} onclose={() => (confirming = false)}>
    <p class="muted">This removes your username, your friends and anything sent to you, straight away. It can't be undone. Your instances and your Minecraft account aren't affected.</p>
    <div class="buttons">
      <button class="btn" onclick={() => (confirming = false)}>Cancel</button>
      <button class="btn primary" onclick={deleteAccount}>Delete account</button>
    </div>
  </Modal>
{/if}

{#if stranded}
  {@const { friend, entry, files } = stranded}
  <Modal title="Send {entry.name}" width={440} onclose={() => (stranded = null)}>
    <p>{files.length === 1 ? "1 file isn't on Modrinth or CurseForge, so it can't be sent:" : `${files.length} files aren't on Modrinth or CurseForge, so they can't be sent:`}</p>
    <p class="muted files">{files.slice(0, 8).join(', ')}{files.length > 8 ? ', …' : ''}</p>
    <p class="muted">Export it as a pack to include them.</p>
    <div class="buttons">
      <button class="btn" onclick={() => sendInstance(friend, entry, true)}>Send without them</button>
      <button class="btn primary" onclick={exportInstead}>Export pack</button>
    </div>
  </Modal>
{/if}

<style>
  .page { height: 100%; overflow-y: auto; }
  .top { height: clamp(110px, 18vh, 150px); }
  .title { position: absolute; right: var(--gutter); bottom: 10px; left: var(--gutter); z-index: 1; }
  .title h1 { font-size: 30px; letter-spacing: -0.025em; }

  .entry { max-width: 360px; padding: 8px var(--gutter) var(--gutter); }
  form:not(.adder) { display: flex; flex-direction: column; gap: 6px; padding-top: 14px; }
  form label { margin-top: 6px; color: var(--text-2); }
  form:not(.adder) .btn { margin-top: 12px; }

  .body { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: 28px; padding: 8px var(--gutter) var(--gutter); }
  .adder { display: flex; gap: 8px; }
  section { margin-top: 20px; }
  aside section:first-child { margin-top: 0; }
  h2 { margin-bottom: 8px; font-size: var(--text-l); }
  .friend, .share { display: flex; align-items: center; gap: 12px; min-height: 60px; box-shadow: inset 0 -1px var(--line); }
  .share { gap: 6px; }
  .face { flex: none; display: block; overflow: hidden; width: 40px; height: 40px; clip-path: var(--stepped); }
  .text { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .name { font-size: var(--text-m); font-weight: 650; }
  .title .name { color: var(--text); }
  .sub { color: var(--text-3); font-size: var(--text-xs); }
  .on { color: var(--good); font-weight: 600; }
  .check { display: flex; align-items: center; gap: 8px; cursor: pointer; }
  .actions { display: flex; gap: 8px; margin-top: 12px; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; padding-top: 16px; }
  .files { margin: 6px 0 10px; overflow-wrap: anywhere; }
</style>

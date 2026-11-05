<script lang="ts">
  import { onMount } from 'svelte';
  import { api, openExternal } from '../lib/api';
  import type { ApiError, DeviceLogin } from '../lib/api';
  import { explain } from '../lib/errors';
  import type { Explained } from '../lib/errors';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  let login = $state<DeviceLogin | null>(null);
  let failure = $state<ApiError | null>(null);
  let explained = $state<Explained | null>(null);
  let copied = $state(false);
  let attempt = 0;

  function close() {
    app.dialog = 'none';
    app.signInReason = null;
  }

  async function begin() {
    const mine = ++attempt;
    failure = null;
    explained = null;
    login = null;
    try {
      const started = await api.beginSignIn();
      if (mine !== attempt) return;
      login = started;
      const account = await api.finishSignIn();
      if (mine !== attempt) return;
      await app.refreshAccounts();
      app.toast('good', `Signed in as ${account.name}`);
      close();
    } catch (error) {
      if (mine !== attempt) return;
      failure = error as ApiError;
      explained = explain(error);
    }
  }

  onMount(begin);

  async function copy() {
    if (!login) return;
    try {
      await navigator.clipboard.writeText(login.userCode);
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch {
      // The code is on screen to type or select.
    }
  }
</script>

<Modal title="Sign in with Microsoft" onclose={close}>
  {#if app.signInReason && !failure}<p class="reason">{app.signInReason}</p>{/if}
  {#if failure?.code === 'app_not_approved'}
    <p>Sign-in isn't available yet. Waymark Launcher is waiting on Mojang to approve it, and there's nothing wrong with your account.</p>
  {:else if failure?.code === 'not_owned'}
    <p>This Microsoft account doesn't own Minecraft: Java Edition.</p>
    <p class="muted more">If you bought the game on another account, sign in with that one instead.</p>
    <div class="actions">
      <button class="btn" onclick={begin}>Use another account</button>
      <button class="btn" onclick={() => openExternal('https://www.minecraft.net/store/minecraft-java-bedrock-edition-pc')}>Open the Minecraft store</button>
    </div>
  {:else if failure}
    <p>Signing in didn't finish.</p>
    <p class="muted more">{explained?.sentence || 'The code may have run out of time, or the sign-in page was closed. Try again for a new code.'}</p>
    {#if failure.message}<p class="faint small technical selectable">{failure.message}</p>{/if}
    <div class="actions"><button class="btn primary" onclick={begin}>Try again</button></div>
  {:else if login}
    <p>Open the Microsoft sign-in page and enter this code.</p>
    <div class="code">
      <span class="mono selectable">{login.userCode}</span>
      <button class="btn" onclick={copy}>{copied ? 'Copied' : 'Copy'}</button>
    </div>
    <div class="actions"><button class="btn primary" onclick={() => openExternal(login!.verificationUri)}>Open sign-in page</button></div>
    <p class="wait faint small">This window closes when you've finished signing in.</p>
  {:else}
    <p class="muted">Connecting to Microsoft</p>
  {/if}
</Modal>

<style>
  .reason { margin-bottom: 12px; color: var(--text-2); }
  .more { margin-top: 6px; }
  .technical { margin-top: 8px; font-family: var(--mono); overflow-wrap: anywhere; }
  .code {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 12px 0;
    padding: 10px 12px;
    background: var(--bg);
    clip-path: var(--stepped);
  }
  .code span { font-size: 20px; letter-spacing: 0.08em; }
  .actions { display: flex; gap: 8px; margin-top: 12px; }
  .wait { margin-top: 12px; }
</style>

<script lang="ts">
  import { onMount } from 'svelte';
  import { api, openExternal } from '../lib/api';
  import type { ApiError, DeviceLogin } from '../lib/api';
  import { app } from '../lib/state.svelte';
  import Modal from './Modal.svelte';

  let login = $state<DeviceLogin | null>(null);
  let failure = $state<ApiError | null>(null);
  let copied = $state(false);

  function close() {
    app.dialog = 'none';
  }

  onMount(async () => {
    try {
      login = await api.beginSignIn();
      const account = await api.finishSignIn();
      await app.refreshAccounts();
      app.toast('good', `Signed in as ${account.name}`);
      close();
    } catch (error) {
      failure = error as ApiError;
    }
  });

  async function copy() {
    if (!login) return;
    await navigator.clipboard.writeText(login.userCode);
    copied = true;
    setTimeout(() => (copied = false), 1600);
  }
</script>

<Modal title="Sign in with Microsoft" onclose={close}>
  {#if failure?.code === 'app_not_approved'}
    <p>Sign-in isn't available yet. Waymark is waiting on Mojang to approve it, and there's nothing wrong with your account.</p>
  {:else if failure?.code === 'not_owned'}
    <p>This Microsoft account doesn't own Minecraft: Java Edition.</p>
    <div class="actions"><button class="btn" onclick={() => openExternal('https://www.minecraft.net/store/minecraft-java-bedrock-edition-pc')}>Open the Minecraft store</button></div>
  {:else if failure}
    <p class="selectable">{failure.message}</p>
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
  .actions { margin-top: 12px; }
  .wait { margin-top: 12px; }
</style>

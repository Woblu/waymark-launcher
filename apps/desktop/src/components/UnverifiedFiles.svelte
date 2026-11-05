<script lang="ts">
  // After a pack installs: program files it carried itself instead of naming
  // them on Modrinth or CurseForge. Packs that carried malware hid it this way,
  // so the player looks before the first launch. Nothing has run yet.
  import Modal from './Modal.svelte';
  import { api } from '../lib/api';
  import { app } from '../lib/state.svelte';

  const SHOWN = 12;
  const pack = $derived(app.unverified);
  let deleting = $state(false);

  function keep() {
    app.unverified = null;
  }

  async function remove() {
    if (!pack) return;
    // Clearing the dialog clears `pack` too, so keep what the messages need.
    const { instance, name } = pack;
    deleting = true;
    try {
      await api.deleteInstance(instance);
      app.unverified = null;
      app.go('library');
      await app.refresh();
      app.toast('good', `Deleted ${name}`, 'It stays in the trash for 30 days.');
    } catch (error) {
      app.fail(error, `Could not delete ${name}`);
    } finally {
      deleting = false;
    }
  }
</script>

{#if pack}
  <Modal title="Check this pack before playing" width={520} onclose={keep}>
    <p class="name">{pack.name}</p>
    <p>{pack.files.length === 1 ? 'This pack brought a program file of its own.' : `This pack brought ${pack.files.length} program files of its own.`}</p>
    <p class="muted">These didn't come from Modrinth or CurseForge, which scan the files they host. Packs that carried malware have hidden it this way. Nothing has run yet.</p>
    <ul class="files selectable mono">
      {#each pack.files.slice(0, SHOWN) as file (file)}<li>{file}</li>{/each}
      {#if pack.files.length > SHOWN}<li class="faint">and {pack.files.length - SHOWN} more</li>{/if}
    </ul>
    <p class="muted">If you trust where the pack came from, keep it. If you don't, delete the instance. It goes to the trash for 30 days.</p>
    <div class="buttons">
      <span class="spacer"></span>
      <button class="btn danger" disabled={deleting} onclick={remove}>Delete instance</button>
      <button class="btn primary" disabled={deleting} onclick={keep}>Keep</button>
    </div>
  </Modal>
{/if}

<style>
  .files { margin: 10px 0 12px; padding: 10px 12px; max-height: 220px; overflow: auto; background: var(--control); font-size: var(--text-s); list-style: none; }
  .files li { overflow-wrap: anywhere; }
  .buttons { display: flex; gap: 8px; margin-top: 16px; }
  .spacer { flex: 1; }
</style>

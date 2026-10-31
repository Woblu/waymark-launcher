<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import type { Account, Settings } from '../lib/api';
  import { bytes } from '../lib/format';
  import { app } from '../lib/state.svelte';

  let settings = $state<Settings | null>(null);
  let reclaiming = $state(false);

  onMount(async () => {
    settings = await api.settings();
  });

  async function save() {
    if (!settings) return;
    try {
      await api.saveSettings($state.snapshot(settings));
    } catch (error) {
      app.fail(error, 'Could not save settings');
    }
  }

  async function activate(account: Account) {
    app.accounts = await api.setActiveAccount(account.uuid);
  }

  async function signOut(account: Account) {
    app.accounts = await api.removeAccount(account.uuid);
  }

  async function reclaim() {
    reclaiming = true;
    try {
      const freed = await api.reclaimSpace();
      app.toast('good', freed ? `Freed ${bytes(freed)}` : 'Nothing to clean up');
    } catch (error) {
      app.fail(error);
    }
    reclaiming = false;
  }
</script>

<div class="page">
  <section>
    <h2>Accounts</h2>
    <div class="body">
      {#each app.accounts as account (account.uuid)}
        <div class="account">
          <span>{account.name}{#if account.active}<span class="faint active">Active</span>{/if}</span>
          {#if !account.active}<button class="btn" onclick={() => activate(account)}>Use this account</button>{/if}
          <button class="btn" onclick={() => signOut(account)}>Sign out</button>
        </div>
      {:else}
        <p class="muted">Sign in with a Microsoft account that owns Minecraft: Java Edition to play.</p>
      {/each}
      <button class="btn add" onclick={() => (app.dialog = 'signin')}>Add an account</button>
    </div>
  </section>

  {#if settings}
    <section>
      <h2>Game</h2>
      <div class="body">
        <div class="form-row">
          <span class="form-label">Default memory</span>
          <div class="slider">
            <input type="range" min="1024" max="32768" step="512" aria-label="Default memory" bind:value={settings.memoryMb} onchange={save} />
            <span class="tabular">{(settings.memoryMb / 1024).toFixed(1)} GB</span>
          </div>
        </div>
        <div class="form-row">
          <span class="form-label">Snapshots</span>
          <label class="check"><input type="checkbox" bind:checked={settings.showSnapshots} onchange={save} /> List snapshot versions when creating an instance</label>
        </div>
        <div class="form-row">
          <span class="form-label">While playing</span>
          <div>
            <label class="check"><input type="checkbox" bind:checked={settings.closeWhilePlaying} onchange={save} /> Close the window to free memory for the game</label>
            <p class="form-hint">The window comes back when the game exits. While it's closed, you can reopen it from the Waymark icon in the system tray.</p>
          </div>
        </div>
      </div>
    </section>

    <section>
      <h2>Updates</h2>
      <div class="body">
        <div class="form-row">
          <label for="delay">Hold back new releases</label>
          <div>
            <select id="delay" class="field short" bind:value={settings.releaseDelayHours} onchange={save}>
              <option value={0}>Off</option>
              <option value={24}>1 day</option>
              <option value={48}>2 days</option>
              <option value={168}>7 days</option>
            </select>
            <p class="form-hint">A mod update won't be offered until it has been out this long, which gives people time to catch malware from a stolen author account.</p>
          </div>
        </div>
      </div>
    </section>
  {/if}

  <section>
    <h2>Storage</h2>
    <div class="body">
      <div class="form-row">
        <span class="form-label">Unused files</span>
        <div>
          <button class="btn" disabled={reclaiming} onclick={reclaim}>{reclaiming ? 'Cleaning up' : 'Clean up'}</button>
          <p class="form-hint">Removes stored files that no instance or waymark needs.</p>
        </div>
      </div>
    </div>
  </section>

  <section>
    <h2>About</h2>
    <div class="body">
      <p>Waymark 0.1.0, made by Woblu. Support: waymarklaunchersupport@proton.me</p>
      <p class="faint legal">Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.</p>
    </div>
  </section>
</div>

<style>
  .page { height: 100%; overflow-y: auto; padding: 0 24px 20px; }
  section {
    display: grid;
    grid-template-columns: 150px minmax(0, 660px);
    gap: 16px;
    padding: 16px 0;
    border-bottom: 1px solid var(--line);
  }
  section:last-child { border-bottom: 0; }
  h2 { font-size: var(--text-s); }
  .body .form-row:first-child { padding-top: 0; }
  .form-row:last-child { padding-bottom: 0; }
  .form-row > .form-label, .form-row > label { padding-top: 6px; }
  .account { display: flex; align-items: center; gap: 8px; padding: 0 0 8px; }
  .account > span { flex: 1; }
  .active { margin-left: 8px; }
  .add { margin-top: 2px; }
  .slider { display: flex; align-items: center; gap: 12px; padding-top: 4px; }
  .slider input { flex: 1; }
  .slider span { width: 56px; text-align: right; }
  .check { display: flex; align-items: center; gap: 7px; padding-top: 6px; color: var(--text); }
  .short { width: 120px; }
  .legal { margin-top: 4px; }
</style>

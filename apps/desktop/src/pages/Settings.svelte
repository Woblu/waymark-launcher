<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import type { Account, Settings, Trashed } from '../lib/api';
  import { bytes, timeAgo } from '../lib/format';
  import { LANGUAGE_NAMES, chooseLanguage, savedLanguage } from '../lib/i18n';
  import { useTheme } from '../lib/theme';
  import { app } from '../lib/state.svelte';

  let settings = $state<Settings | null>(null);
  let reclaiming = $state(false);
  let deleted = $state<Trashed[]>([]);

  async function loadDeleted() {
    deleted = await api.trashed().catch(() => []);
  }

  onMount(async () => {
    settings = await api.settings();
  });

  $effect(() => {
    app.contentTick;
    loadDeleted();
  });

  async function restore(item: Trashed) {
    await app.restore(item.id);
    await loadDeleted();
  }

  async function erase(item: Trashed) {
    try {
      await api.eraseTrashed(item.id);
      await loadDeleted();
    } catch (error) {
      app.fail(error);
    }
  }

  async function save() {
    if (!settings) return;
    try {
      await api.saveSettings($state.snapshot(settings));
    } catch (error) {
      app.fail(error, 'Could not save settings');
    }
  }

  let language = $state(savedLanguage());

  async function setLanguage() {
    if (settings) {
      settings.language = language;
      await save();
    }
    chooseLanguage(language);
  }

  async function chooseTheme() {
    if (!settings) return;
    useTheme(settings.theme);
    await save();
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

  let checking = $state(false);
  let upToDate = $state(false);

  async function checkNow() {
    checking = true;
    upToDate = false;
    try {
      const found = await api.checkForUpdate();
      if (found) app.update = found;
      else upToDate = true;
    } catch (error) {
      app.fail(error, 'Could not check for updates');
    }
    checking = false;
  }
</script>

<div class="page">
  <div class="column">
  <h1>Settings</h1>
  {#if settings}
    <section>
      <h2>Appearance</h2>
      <div class="body">
        <div class="form-row">
          <label for="theme">Theme</label>
          <select id="theme" class="field short" bind:value={settings.theme} onchange={chooseTheme}>
            <option value="system">Same as this computer</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </div>
        <div class="form-row">
          <label for="language">Language</label>
          <div>
            <select id="language" class="field short" bind:value={language} onchange={setLanguage}>
              <option value="system">Same as this computer</option>
              {#each Object.entries(LANGUAGE_NAMES) as [code, name] (code)}<option value={code}>{name}</option>{/each}
            </select>
          </div>
        </div>
      </div>
    </section>
  {/if}

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
          <span class="form-label">Waymark</span>
          <div>
            <label class="check top"><input type="checkbox" bind:checked={settings.autoUpdate} onchange={save} /> Keep Waymark up to date</label>
            <p class="form-hint">New versions download in the background and install when you close Waymark. It never restarts during a game.</p>
            <div class="version">
              <span class="tabular">Version {app.version}</span>
              {#if app.update}
                <button class="btn" onclick={() => app.installUpdate()}>Restart to update</button>
              {:else}
                <button class="btn" disabled={checking} onclick={checkNow}>{checking ? 'Checking' : 'Check for updates'}</button>
                {#if upToDate}<span class="faint">You're up to date</span>{/if}
              {/if}
            </div>
          </div>
        </div>
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

  {#if deleted.length}
    <section>
      <h2>Recently deleted</h2>
      <div class="body">
        <p class="form-hint first">Kept for 30 days after they're deleted.</p>
        {#each deleted as item (item.id)}
          <div class="account">
            <span>{item.name}<span class="faint active">{item.kind === 'world' ? `World from ${item.instance}` : item.kind === 'screenshot' ? `Screenshot from ${item.instance}` : 'Instance'}, deleted {timeAgo(item.at)}</span></span>
            <button class="btn" onclick={() => restore(item)}>Restore</button>
            <button class="btn danger" onclick={() => erase(item)}>Delete now</button>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <section>
    <h2>About</h2>
    <div class="body">
      <p>Waymark {app.version}, made by Woblu. Support: waymarklaunchersupport@proton.me</p>
      <p class="faint legal">Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.</p>
      <p class="faint legal">Skin name tags use Monocraft by Idrees Hassan, under the SIL Open Font License.</p>
    </div>
  </section>
  </div>
</div>

<style>
  .page { height: 100%; overflow-y: auto; padding: 20px var(--gutter) var(--gutter); }
  .column { max-width: 860px; }
  h1 { margin-bottom: 8px; }
  section { display: grid; grid-template-columns: 150px minmax(0, 1fr); gap: 20px; padding: 18px 0; border-bottom: 1px solid var(--line); }
  section:last-child { border-bottom: 0; }
  h2 { padding-top: 8px; font-size: var(--text-m); }
  .body .form-row:first-child { padding-top: 0; }
  .form-row:last-child { padding-bottom: 0; }
  .account { display: flex; align-items: center; gap: 8px; padding: 6px 0; }
  .account > span { flex: 1; min-width: 0; font-weight: 500; }
  .active { margin-left: 8px; font-weight: 400; }
  .add { margin-top: 6px; }
  .slider { display: flex; align-items: center; gap: 12px; padding-top: 6px; }
  .slider input { flex: 1; }
  .slider span { width: 56px; text-align: right; }
  .check { display: flex; align-items: center; gap: 8px; padding-top: 8px; color: var(--text); }
  .short { width: 240px; }
  .check.top { padding-top: 8px; }
  .version { display: flex; align-items: center; gap: 12px; margin-top: 12px; }
  .legal { margin-top: 4px; }
  .first { margin: 0 0 8px; }
</style>

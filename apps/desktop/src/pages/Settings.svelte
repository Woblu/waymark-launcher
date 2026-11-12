<script lang="ts">
  import { onMount } from 'svelte';
  import { Plus } from '@lucide/svelte';
  import Modal from '../components/Modal.svelte';
  import SettingsLayout from '../components/SettingsLayout.svelte';
  import StarterMods from '../components/StarterMods.svelte';
  import SharedSettings from '../components/SharedSettings.svelte';
  import { api, openFolder } from '../lib/api';
  import type { Account, CustomTheme, Settings, Trashed } from '../lib/api';
  import { asSentence, explain } from '../lib/errors';
  import type { Explained } from '../lib/errors';
  import { bytes, number, timeAgo } from '../lib/format';
  import { LANGUAGE_NAMES, chooseLanguage, savedLanguage } from '../lib/i18n';
  import { PRESETS, previewTheme, useTheme } from '../lib/theme';
  import { EVERY_VERSION, app } from '../lib/state.svelte';

  const SECTIONS = [
    { id: 'appearance', label: 'Appearance' },
    { id: 'accounts', label: 'Accounts' },
    { id: 'game', label: 'Game' },
    { id: 'instances', label: 'New instances' },
    { id: 'shared', label: 'Shared settings' },
    { id: 'updates', label: 'Updates' },
    { id: 'storage', label: 'Storage' },
    { id: 'help', label: 'Help' },
    { id: 'about', label: 'About' },
  ];

  let settings = $state<Settings | null>(null);
  let reclaiming = $state(false);
  let deleted = $state<Trashed[]>([]);

  async function loadDeleted() {
    deleted = await api.trashed().catch(() => []);
  }

  /** This build has a Discord application to show presence as. */
  let discordAvailable = $state(false);

  /** Why settings couldn't be read, shown where they'd be with a way to try again. */
  let loadProblem = $state<Explained | null>(null);

  async function loadSettings() {
    loadProblem = null;
    try {
      settings = await api.settings();
    } catch (error) {
      loadProblem = explain(error);
    }
  }

  onMount(async () => {
    await loadSettings();
    discordAvailable = await api.discordAvailable().catch(() => false);
  });

  $effect(() => {
    app.contentTick;
    loadDeleted();
  });

  async function restore(item: Trashed) {
    await app.restore(item.id);
    await loadDeleted();
  }

  /** Deleting for good can't be undone, so it's asked about first. */
  let erasing = $state<Trashed | null>(null);

  async function erase(item: Trashed) {
    erasing = null;
    try {
      await api.eraseTrashed(item.id);
      await loadDeleted();
    } catch (error) {
      app.fail(error, `Could not delete ${item.name}`);
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
    useTheme(settings.theme, settings.customThemes);
    await save();
  }

  let editing = $state<CustomTheme | null>(null);
  let creating = $state(false);

  const ownTheme = $derived(settings?.customThemes.find((theme) => theme.id === settings?.theme));

  /** Every theme with the two colors it's made from. The computer's own is half of each. */
  const themeChoices = $derived([
    { id: 'system', name: 'Same as this computer', base: 'linear-gradient(135deg, #0c1118 50%, #f3efe7 50%)', accent: '#efb36c' },
    { id: 'dark', name: 'Dark', base: '#0c1118', accent: '#efb36c' },
    { id: 'light', name: 'Light', base: '#f3efe7', accent: '#a85f10' },
    ...[...PRESETS, ...(settings?.customThemes ?? [])].map((theme) => ({ id: theme.id, name: theme.name, base: theme.base, accent: theme.accent })),
  ]);

  async function pickTheme(id: string) {
    if (!settings) return;
    settings.theme = id;
    await chooseTheme();
  }

  function newTheme() {
    creating = true;
    editing = { id: crypto.randomUUID(), name: '', base: '#0c1118', accent: '#efb36c' };
  }

  function editTheme(theme: CustomTheme) {
    creating = false;
    editing = { ...theme };
  }

  $effect(() => {
    if (editing) previewTheme({ base: editing.base, accent: editing.accent });
  });

  function stopEditing() {
    editing = null;
    if (settings) useTheme(settings.theme, settings.customThemes);
  }

  async function saveTheme(event: SubmitEvent) {
    event.preventDefault();
    if (!settings || !editing) return;
    const theme = { ...$state.snapshot(editing), name: editing.name.trim() };
    settings.customThemes = [...settings.customThemes.filter((other) => other.id !== theme.id), theme];
    settings.theme = theme.id;
    stopEditing();
    await save();
  }

  async function deleteTheme() {
    if (!settings || !editing) return;
    const id = editing.id;
    settings.customThemes = settings.customThemes.filter((other) => other.id !== id);
    if (settings.theme === id) settings.theme = 'dark';
    stopEditing();
    await save();
  }

  async function activate(account: Account) {
    try {
      app.accounts = await api.setActiveAccount(account.uuid);
    } catch (error) {
      app.fail(error, `Could not switch to ${account.name}`);
    }
  }

  async function signOut(account: Account) {
    try {
      app.accounts = await api.removeAccount(account.uuid);
    } catch (error) {
      app.fail(error, `Could not sign out ${account.name}`);
    }
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

  let choosingShared = $state(false);
  /** The instance the shared set started from, once there is one. */
  let sharedFrom = $state<string | null>(null);

  async function loadShared() {
    const found = await api.sharedSettings().catch(() => null);
    sharedFrom = found?.sets.find((set) => set.from)?.from ?? null;
  }

  $effect(() => {
    if (settings?.shareSettings) loadShared();
  });

  async function toggleSharing() {
    await save();
    if (settings?.shareSettings) await loadShared();
  }

  let editingStarters = $state(false);
  /** Each version with a list, the shared one first, then the newest. */
  const starterLists = $derived(
    Object.entries(settings?.starters ?? {})
      .filter(([, items]) => items.length)
      .sort(([a], [b]) => (a === EVERY_VERSION ? -1 : b === EVERY_VERSION ? 1 : b.localeCompare(a, undefined, { numeric: true }))),
  );

  let checking = $state(false);
  let upToDate = $state(false);

  let copiedDiagnostics = $state(false);

  /** Everything a support email needs, with folder names and tokens taken out. */
  async function copyDiagnostics() {
    try {
      const report = await api.diagnosticsReport();
      await navigator.clipboard.writeText(report ?? `Waymark Launcher ${app.version}`);
      copiedDiagnostics = true;
      setTimeout(() => (copiedDiagnostics = false), 2500);
    } catch (error) {
      app.fail(error, 'Could not copy diagnostics');
    }
  }

  async function openLogFolder() {
    try {
      const diagnostics = await api.launcherDiagnostics();
      if (diagnostics) await openFolder(diagnostics.folder);
    } catch (error) {
      app.fail(error, 'Could not open the log folder');
    }
  }

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
  <SettingsLayout title="Settings" sections={SECTIONS} bind:selected={app.settingsSection}>
    {#if loadProblem}
      <div class="notice trouble" role="alert">
        <span><strong>Your settings didn't load.</strong> {loadProblem.sentence || asSentence(loadProblem.technical)}</span>
        <button class="btn" onclick={loadSettings}>Try again</button>
      </div>
    {/if}

    {#if app.settingsSection === 'appearance' && settings}
      <h1 class="section-title">Appearance</h1>
      <h2 class="group-title">Theme</h2>
      <div class="group">
        <div class="lone">
          <div class="swatches" role="radiogroup" aria-label="Theme">
            {#each themeChoices as choice (choice.id)}
              <button class="swatch" role="radio" aria-checked={settings.theme === choice.id} class:chosen={settings.theme === choice.id} class:wide={choice.id === 'system'} onclick={() => pickTheme(choice.id)}>
                <span class="chip" style:background={choice.base}><span class="dot" style:background={choice.accent}></span></span>
                <span class="truncate">{choice.name}</span>
              </button>
            {/each}
          </div>
          <div class="actions">
            {#if ownTheme}<button class="btn" onclick={() => editTheme(ownTheme)}>Edit</button>{/if}
            <button class="btn" onclick={newTheme}><Plus size={16} /> New theme</button>
          </div>
        </div>
      </div>
      <h2 class="group-title">Language</h2>
      <div class="group">
        <div class="lone">
          <select id="language" class="field short" aria-label="Language" bind:value={language} onchange={setLanguage}>
            <option value="system">Same as this computer</option>
            {#each Object.entries(LANGUAGE_NAMES) as [code, name] (code)}<option value={code}>{name}</option>{/each}
          </select>
        </div>
      </div>
    {:else if app.settingsSection === 'accounts'}
      <h1 class="section-title">Accounts</h1>
      <div class="group">
        {#each app.accounts as account (account.uuid)}
          <div class="account">
            <span class="who">{account.name}{#if account.active}<span class="badge">Active</span>{/if}</span>
            {#if !account.active}<button class="btn" onclick={() => activate(account)}>Use this account</button>{/if}
            <button class="btn" onclick={() => signOut(account)}>Sign out</button>
          </div>
        {:else}
          <p class="muted">Sign in with a Microsoft account that owns Minecraft: Java Edition to play.</p>
        {/each}
      </div>
      <div class="after"><button class="btn" onclick={() => (app.dialog = 'signin')}><Plus size={16} /> Add an account</button></div>
    {:else if app.settingsSection === 'game' && settings}
      <h1 class="section-title">Game</h1>
      <div class="group">
        <div class="form-row">
          <span class="form-label">Default memory</span>
          <div class="slider">
            <input type="range" min="1024" max="32768" step="512" aria-label="Default memory" bind:value={settings.memoryMb} onchange={save} />
            <span class="tabular">{number(settings.memoryMb / 1024, 1)} GB</span>
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
            <p class="form-hint">The window comes back when the game exits. While it's closed, you can reopen it from the Waymark Launcher icon in the system tray.</p>
          </div>
        </div>
        {#if discordAvailable}
          <div class="form-row">
            <span class="form-label">Discord</span>
            <div>
              <label class="check"><input type="checkbox" bind:checked={settings.discordPresence} onchange={save} /> Show what you're playing on your Discord profile</label>
              <p class="form-hint">Your Discord friends see the instance's name, its Minecraft version, and the server's name when it's from the server list. Waymark Launcher tells the Discord app on this computer, and never shares a private server's address.</p>
            </div>
          </div>
        {/if}
      </div>
    {:else if app.settingsSection === 'instances' && settings}
      <h1 class="section-title">New instances</h1>
      <div class="group">
        <div class="form-row">
          <span class="form-label">Starter mods</span>
          <div>
            <button class="btn" onclick={() => (editingStarters = true)}>Edit starter mods</button>
            <p class="form-hint">Mods, resource packs and shaders added to each new instance on a Minecraft version.</p>
            {#if starterLists.length}
              <p class="lists">
                {#each starterLists as [key, items] (key)}<span class="tabular">{key === EVERY_VERSION ? `Every version (${items.length})` : `${key} (${items.length})`}</span>{/each}
              </p>
            {/if}
          </div>
        </div>
      </div>
    {:else if app.settingsSection === 'shared' && settings}
      <h1 class="section-title">Shared settings</h1>
      <div class="group">
        <div class="form-row">
          <span class="form-label">Game settings</span>
          <div>
            <label class="check"><input type="checkbox" bind:checked={settings.shareSettings} onchange={toggleSharing} /> Share game settings between instances</label>
            <p class="form-hint">Keybinds, video, sound and other options carry over. What you change in one instance reaches the others the next time they start. Minecraft 1.13 and newer share one set and older releases another. An instance can keep its own from its settings.</p>
            {#if settings.shareSettings}
              <label class="check"><input type="checkbox" bind:checked={settings.shareServers} onchange={save} /> Share the server list too</label>
              <div class="version">
                <button class="btn" onclick={() => (choosingShared = true)}>Choose settings</button>
                {#if sharedFrom}<span class="faint">{`Started from ${sharedFrom}`}</span>{/if}
              </div>
            {/if}
          </div>
        </div>
      </div>
    {:else if app.settingsSection === 'updates' && settings}
      <h1 class="section-title">Updates</h1>
      <h2 class="group-title">Waymark Launcher</h2>
      <div class="group">
        <div class="form-row">
          <span class="form-label">Version</span>
          <div class="version first">
            <span class="tabular">{app.version}</span>
            {#if app.update}
              <button class="btn primary" onclick={() => app.installUpdate()}>Restart to update</button>
            {:else}
              <button class="btn" disabled={checking} onclick={checkNow}>{checking ? 'Checking' : 'Check for updates'}</button>
              {#if upToDate}<span class="faint">You're up to date</span>{/if}
            {/if}
          </div>
        </div>
        <div class="form-row">
          <span class="form-label">Updates</span>
          <div>
            <label class="check"><input type="checkbox" bind:checked={settings.autoUpdate} onchange={save} /> Keep Waymark Launcher up to date</label>
            <p class="form-hint">New versions download in the background and install when you close Waymark Launcher. It never restarts during a game.</p>
            <label class="check"><input type="checkbox" bind:checked={settings.betaUpdates} disabled={!settings.autoUpdate} onchange={save} /> Get beta versions early</label>
            <p class="form-hint">Betas get new features a few days before everyone else, and sometimes new bugs. Turn this off and you move to the next full release when it comes out.</p>
          </div>
        </div>
      </div>
      <h2 class="group-title">Mods and packs</h2>
      <div class="group">
        <div class="form-row">
          <span class="form-label">After an update</span>
          <div>
            <label class="check"><input type="checkbox" bind:checked={settings.checkUpdates} onchange={save} /> Check that the game still starts after an update</label>
            <p class="form-hint">Waymark Launcher starts the game out of sight after updating mods or a pack, and undoes the update if it won't load. Pressing Play stops the check.</p>
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
    {:else if app.settingsSection === 'storage'}
      <h1 class="section-title">Storage</h1>
      <div class="group">
        <div class="form-row">
          <span class="form-label">Unused files</span>
          <div>
            <button class="btn" disabled={reclaiming} onclick={reclaim}>{reclaiming ? 'Cleaning up' : 'Clean up'}</button>
            <p class="form-hint">Removes stored files that no instance or waymark needs.</p>
          </div>
        </div>
      </div>
      <h2 class="group-title">Recently deleted</h2>
      <div class="group">
        {#each deleted as item (item.id)}
          <div class="account">
            <span class="who">{item.name}<span class="faint detail">{item.kind === 'world' ? `World from ${item.instance}` : item.kind === 'screenshot' ? `Screenshot from ${item.instance}` : 'Instance'}, deleted {timeAgo(item.at)}</span></span>
            <button class="btn" onclick={() => restore(item)}>Restore</button>
            <button class="btn danger" onclick={() => (erasing = item)}>Delete now</button>
          </div>
        {:else}
          <p class="muted">Nothing deleted lately. Deleted instances, worlds and screenshots are kept here for 30 days.</p>
        {/each}
      </div>
      {#if deleted.length}<p class="form-hint">Kept for 30 days after they're deleted.</p>{/if}
    {:else if app.settingsSection === 'help'}
      <h1 class="section-title">Help</h1>
      <div class="group">
        <div class="form-row">
          <span class="form-label">Diagnostics</span>
          <div>
            <div class="version first">
              <button class="btn" onclick={copyDiagnostics}>{copiedDiagnostics ? 'Copied' : 'Copy diagnostics'}</button>
              <button class="btn" onclick={openLogFolder}>Open log folder</button>
            </div>
            <p class="form-hint">Diagnostics are the version, your system and the end of Waymark Launcher's own log, with folder names and sign-in tokens taken out. Nothing is sent anywhere by itself.</p>
            <p class="form-hint">Paste them into a GitHub issue or an email to <span class="selectable">waymarklaunchersupport@proton.me</span></p>
          </div>
        </div>
      </div>
    {:else if app.settingsSection === 'about'}
      <h1 class="section-title">About</h1>
      <div class="group">
        <div class="lone">
          <p>Waymark Launcher {app.version}, made by Woblu. Support: waymarklaunchersupport@proton.me</p>
          <p class="faint legal">Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.</p>
          <p class="faint legal">Skin name tags use Monocraft by Idrees Hassan, under the SIL Open Font License.</p>
        </div>
      </div>
    {/if}
  </SettingsLayout>
</div>

{#if choosingShared && settings}
  <SharedSettings bind:settings onsave={save} onclose={() => (choosingShared = false)} />
{/if}

{#if editingStarters && settings}
  <StarterMods bind:settings onsave={save} onclose={() => (editingStarters = false)} />
{/if}

{#if erasing}
  {@const item = erasing}
  <Modal title="Delete for good" width={420} onclose={() => (erasing = null)}>
    <p class="erasing name">{item.name}</p>
    <p class="muted">
      {item.kind === 'instance'
        ? "This instance and its worlds will be deleted for good. This can't be undone."
        : item.kind === 'world'
          ? "This world will be deleted for good. This can't be undone."
          : "This screenshot will be deleted for good. This can't be undone."}
    </p>
    <div class="buttons">
      <span class="spacer"></span>
      <button class="btn" onclick={() => (erasing = null)}>Cancel</button>
      <button class="btn danger" onclick={() => erase(item)}>Delete for good</button>
    </div>
  </Modal>
{/if}

{#if editing}
  <Modal title={creating ? 'New theme' : 'Edit theme'} width={420} onclose={stopEditing}>
    <form onsubmit={saveTheme}>
      <div class="form-row">
        <label for="theme-name">Name</label>
        <input id="theme-name" class="field" required bind:value={editing.name} />
      </div>
      <div class="form-row">
        <label for="theme-base">Background</label>
        <div class="color"><input id="theme-base" type="color" bind:value={editing.base} /><span class="mono">{editing.base}</span></div>
      </div>
      <div class="form-row">
        <label for="theme-accent">Accent</label>
        <div class="color"><input id="theme-accent" type="color" bind:value={editing.accent} /><span class="mono">{editing.accent}</span></div>
      </div>
      <p class="form-hint">The rest of the colors come from these two, and the app shows them as you pick.</p>
      <div class="buttons">
        {#if !creating}<button type="button" class="btn danger" onclick={deleteTheme}>Delete</button>{/if}
        <span class="spacer"></span>
        <button type="button" class="btn" onclick={stopEditing}>Cancel</button>
        <button class="btn primary">Save</button>
      </div>
    </form>
  </Modal>
{/if}

<style>
  .page { height: 100%; }
  .trouble { margin-bottom: 14px; }
  .lone { padding: 16px 0; }
  .short { width: 260px; }

  /* Themes as small postcards of their own colors, the chosen one outlined. */
  .swatches { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .swatch { display: flex; align-items: center; gap: 10px; min-width: 0; padding: 8px 10px 8px 8px; background: var(--control); clip-path: var(--stepped); color: var(--text); font-weight: 500; text-align: left; transition: background-color 100ms ease-out; }
  .swatch:hover { background: var(--control-hover); }
  .swatch.chosen { outline: 2px solid var(--accent); outline-offset: -2px; }
  .swatch.wide { grid-column: span 2; }
  .chip { position: relative; flex: none; width: 34px; height: 24px; clip-path: var(--stepped); }
  .dot { position: absolute; right: 5px; bottom: 5px; width: 10px; height: 10px; }
  .actions { display: flex; gap: 8px; margin-top: 12px; }

  .account { display: flex; align-items: center; gap: 8px; padding: 12px 0; border-top: 1px solid var(--line); }
  .account:first-child { border-top: 0; }
  .who { flex: 1; min-width: 0; font-weight: 550; overflow-wrap: anywhere; }
  .badge { margin-left: 10px; padding: 2px 8px; background: var(--control); clip-path: var(--stepped); color: var(--accent-text); font-size: var(--text-xs); font-weight: 600; }
  .detail { display: block; margin-top: 2px; font-weight: 400; }
  .group > .muted { padding: 14px 0; }
  .after { margin-top: 12px; }

  .slider { display: flex; align-items: center; gap: 12px; padding-top: 6px; }
  .slider input { flex: 1; }
  .slider span { width: 56px; text-align: right; }
  .check { display: flex; align-items: center; gap: 8px; padding-top: 8px; color: var(--text); }
  .check + .form-hint + .check { margin-top: 10px; }
  .version { display: flex; flex-wrap: wrap; align-items: center; gap: 12px; margin-top: 12px; }
  .version.first { margin-top: 2px; }
  .lists { display: flex; flex-wrap: wrap; gap: 6px 14px; margin-top: 8px; }
  .legal { margin-top: 6px; }

  .color { display: flex; align-items: center; gap: 10px; }
  .color input { width: 44px; height: 36px; padding: 0; border: 0; background: none; clip-path: var(--stepped); cursor: pointer; }
  .color input::-webkit-color-swatch-wrapper { padding: 0; }
  .color input::-webkit-color-swatch { border: 0; }
  .color input:focus-visible { outline-offset: -3px; }
  .buttons { display: flex; gap: 8px; padding-top: 14px; }
  .spacer { flex: 1; }
  .erasing { margin-bottom: 6px; font-weight: 600; overflow-wrap: anywhere; }
</style>

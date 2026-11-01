<script lang="ts">
  import { onMount } from 'svelte';
  import { Check, Pencil, Plus, RotateCcw } from '@lucide/svelte';
  import Modal from '../components/Modal.svelte';
  import SkinModel from '../components/SkinModel.svelte';
  import { api, inTauri, localImage, pickSkinFile } from '../lib/api';
  import type { DefaultSkin, SavedSkin, SkinProfile, SkinVariant, Wear } from '../lib/api';
  import { detectVariant, skinPicture } from '../lib/skin-render';
  import type { SkinPicture } from '../lib/skin-render';
  import { app } from '../lib/state.svelte';

  /** Something that can be worn, from the library or the game's own skins. */
  interface Choice {
    key: string;
    name: string;
    url: string;
    variant: SkinVariant;
    wear: Wear;
    saved?: SavedSkin;
  }

  const STEVE = 'https://textures.minecraft.net/texture/31f477eb1a7beee631c2ca64d06f8f68fa93a3386d04452ab27f43acdf1b60cb';

  let saved = $state<SavedSkin[]>([]);
  let defaults = $state<DefaultSkin[]>([]);
  let profile = $state<SkinProfile | null>(null);
  let selected = $state<Choice | null>(null);
  let pictures = $state<Record<string, SkinPicture>>({});
  let applying = $state(false);
  let editing = $state<SavedSkin | null>(null);
  let editName = $state('');
  let editVariant = $state<SkinVariant>('classic');

  const savedUrl = (skin: SavedSkin) => localImage(`skin/${skin.id}.png`) ?? '';
  const savedChoices = $derived(
    saved.map((skin): Choice => ({ key: skin.id, name: skin.name, url: savedUrl(skin), variant: skin.variant, wear: { kind: 'saved', id: skin.id }, saved: skin })),
  );
  const defaultChoices = $derived(
    defaults.map((skin): Choice => ({ key: skin.url, name: skin.variant === 'slim' ? `${skin.name}, slim` : `${skin.name}, wide`, url: skin.url, variant: skin.variant, wear: { kind: 'default', url: skin.url, variant: skin.variant } })),
  );

  const worn = $derived(profile?.skins.find((skin) => skin.state === 'ACTIVE'));
  const wornCape = $derived(profile?.capes.find((cape) => cape.state === 'ACTIVE'));
  const wornVariant = $derived<SkinVariant>(worn?.variant === 'SLIM' ? 'slim' : 'classic');
  const showing = $derived(selected ?? { url: worn?.url ?? STEVE, variant: wornVariant });
  const pending = $derived(!!selected && !isWorn(selected));
  const signedIn = $derived(!!app.activeAccount);
  const name = $derived(profile?.name ?? app.activeAccount?.name ?? 'Player');

  /** Mojang rehosts uploads, so a saved skin's address never matches and only a default can be recognised. */
  function isWorn(choice: Choice): boolean {
    return choice.url === worn?.url && choice.variant === wornVariant;
  }

  async function draw(choices: Choice[]) {
    for (const choice of choices) {
      if (pictures[choice.key]) continue;
      skinPicture(choice.url, choice.variant).then(
        (picture) => (pictures[choice.key] = picture),
        () => {},
      );
    }
  }

  $effect(() => {
    draw([...savedChoices, ...defaultChoices]);
  });

  async function load() {
    const library = await api.skinLibrary();
    saved = library.saved;
    defaults = library.defaults;
  }

  onMount(async () => {
    try {
      await load();
    } catch (error) {
      app.fail(error, 'Could not read your skins');
    }
    profile = await api.skinProfile().catch(() => null);
  });

  async function addSkin() {
    const path = await pickSkinFile();
    if (path) await addFrom(path);
  }

  $effect(() => {
    const dropped = app.droppedSkins;
    if (!dropped.length) return;
    app.droppedSkins = [];
    (async () => {
      for (const path of dropped) await addFrom(path);
    })();
  });

  async function addFrom(path: string) {
    try {
      const skin = await api.addSkin(path);
      const variant = await detectVariant(savedUrl(skin)).catch((): SkinVariant => 'classic');
      saved = await api.updateSkin(skin.id, skin.name, variant);
      startEditing({ ...skin, variant });
    } catch (error) {
      app.fail(error, 'Could not add that skin');
    }
  }

  function startEditing(skin: SavedSkin) {
    editing = skin;
    editName = skin.name;
    editVariant = skin.variant;
  }

  async function saveEdit(event: SubmitEvent) {
    event.preventDefault();
    if (!editing) return;
    const id = editing.id;
    try {
      saved = await api.updateSkin(id, editName.trim() || editing.name, editVariant);
      delete pictures[id];
      editing = null;
      if (selected?.key === id) selected = savedChoices.find((choice) => choice.key === id) ?? null;
    } catch (error) {
      app.fail(error, 'Could not save that skin');
    }
  }

  async function deleteSkin() {
    if (!editing) return;
    try {
      saved = await api.removeSkin(editing.id);
      if (selected?.key === editing.id) selected = null;
      editing = null;
    } catch (error) {
      app.fail(error, 'Could not delete that skin');
    }
  }

  async function apply() {
    if (!selected) return;
    applying = true;
    try {
      profile = await api.wearSkin(selected.wear);
      app.toast('good', `Now wearing ${selected.name}`, 'The game shows your new skin next time you join a world.');
      selected = null;
    } catch (error) {
      app.fail(error, 'Could not change your skin');
    }
    applying = false;
  }

  async function chooseCape(event: Event) {
    const value = (event.currentTarget as HTMLSelectElement).value;
    try {
      profile = await api.wearCape(value || null);
    } catch (error) {
      app.fail(error, 'Could not change your cape');
    }
  }
</script>

{#snippet tile(choice: Choice)}
  {@const picture = pictures[choice.key]}
  <div class="tile" class:selected={selected?.key === choice.key} class:worn={isWorn(choice)}>
    <button class="pick" title={choice.name} aria-pressed={selected?.key === choice.key} aria-label="Preview {choice.name}" onclick={() => (selected = choice)}>
      {#if picture}
        <img class="front" src={picture.front} alt="" />
        <img class="back" src={picture.back} alt="" />
      {:else}
        <span class="waiting"></span>
      {/if}
    </button>
    {#if selected?.key === choice.key}<span class="tick"><Check size={13} strokeWidth={3} /></span>{/if}
    {#if choice.saved}
      <button class="edit" aria-label="Edit {choice.name}" title="Edit" onclick={() => startEditing(choice.saved!)}><Pencil size={13} /></button>
    {/if}
    <span class="label truncate">{choice.name}</span>
  </div>
{/snippet}

<div class="page">
  <div class="layout">
    <div class="preview">
      <h1>Skins</h1>
      <SkinModel skin={showing.url} variant={showing.variant} cape={wornCape?.url} {name} />
      <div class="actions">
        {#if pending}
          <button class="btn" disabled={applying} onclick={() => (selected = null)}><RotateCcw size={15} /> Reset</button>
          <button class="btn primary" disabled={!signedIn || applying} title={signedIn ? undefined : 'Sign in to wear a skin'} onclick={apply}>
            <Check size={15} /> {applying ? 'Applying' : 'Apply'}
          </button>
        {:else if selected?.saved}
          <button class="btn" onclick={() => startEditing(selected!.saved!)}><Pencil size={14} /> Edit skin</button>
        {/if}
      </div>
      {#if profile?.capes.length}
        <div class="cape">
          <label for="cape" class="muted">Cape</label>
          <select id="cape" class="field" value={wornCape?.id ?? ''} onchange={chooseCape}>
            <option value="">None</option>
            {#each profile.capes as cape (cape.id)}<option value={cape.id}>{cape.alias ?? 'Cape'}</option>{/each}
          </select>
        </div>
      {/if}
    </div>

    <div class="lists">
      <section>
        <h2>Saved skins</h2>
        <div class="grid">
          <button class="tile add" disabled={!inTauri} onclick={addSkin}>
            <Plus size={24} />
            <span class="add-label">Add a skin</span>
            <span class="faint small">PNG file</span>
          </button>
          {#each savedChoices as choice (choice.key)}{@render tile(choice)}{/each}
        </div>
      </section>

      <section>
        <h2>Default skins</h2>
        <div class="grid">
          {#each defaultChoices as choice (choice.key)}{@render tile(choice)}{/each}
        </div>
      </section>

      {#if !signedIn}
        <div class="notice">
          <div>
            <strong>You can try on skins without signing in</strong>
            <p class="muted">Sign in to wear one in the game.</p>
          </div>
          <button class="btn" onclick={() => (app.dialog = 'signin')}>Sign in</button>
        </div>
      {/if}
    </div>
  </div>
</div>

{#if editing}
  <Modal title="Edit skin" width={380} onclose={() => (editing = null)}>
    <form onsubmit={saveEdit}>
      <div class="form-row">
        <label for="skin-name">Name</label>
        <input id="skin-name" class="field" bind:value={editName} />
      </div>
      <div class="form-row">
        <span class="form-label">Arms</span>
        <div class="arms">
          <label class="check"><input type="radio" name="arms" value="classic" bind:group={editVariant} /> Wide</label>
          <label class="check"><input type="radio" name="arms" value="slim" bind:group={editVariant} /> Slim</label>
        </div>
      </div>
      <div class="buttons">
        <button type="button" class="btn danger" onclick={deleteSkin}>Delete</button>
        <span class="spacer"></span>
        <button type="button" class="btn" onclick={() => (editing = null)}>Cancel</button>
        <button class="btn primary">Save</button>
      </div>
    </form>
  </Modal>
{/if}

<style>
  .page { position: relative; height: 100%; overflow-y: auto; }
  .layout { display: grid; grid-template-columns: minmax(320px, 1fr) minmax(0, 2fr); gap: 28px; min-height: 100%; padding: 8px var(--gutter) 14px; }
  .preview { position: sticky; top: 8px; display: flex; flex-direction: column; align-self: start; height: calc(100vh - var(--titlebar) - var(--playbar) - 24px); min-height: 380px; }
  h1 { position: absolute; top: 14px; left: 16px; z-index: 1; color: #f0ebe1; pointer-events: none; }
  .actions { position: absolute; right: 0; bottom: 14px; left: 0; z-index: 1; display: flex; justify-content: center; gap: 8px; }
  .cape { flex: none; display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 10px; align-items: center; margin-top: 10px; }

  .lists { min-width: 0; padding-top: 6px; }
  section + section { margin-top: 24px; }
  h2 { margin-bottom: 10px; font-size: var(--text-l); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(112px, 1fr)); gap: 10px; }

  .tile {
    position: relative;
    aspect-ratio: 5 / 7;
    overflow: hidden;
    background: var(--panel);
    clip-path: var(--stepped);
    outline: 2px solid transparent;
    outline-offset: -2px;
    transition: background-color 100ms ease-out;
  }
  .tile:hover { background: var(--raised); }
  .tile.selected { outline-color: var(--accent); }
  .tile.worn { outline-color: var(--good); }
  .pick { position: absolute; inset: 0; display: block; width: 100%; height: 100%; }
  .pick img { position: absolute; inset: 6px; width: calc(100% - 12px); height: calc(100% - 12px); object-fit: contain; }
  .back { opacity: 0; }
  .pick:hover .front, .pick:focus-visible .front { opacity: 0; }
  .pick:hover .back, .pick:focus-visible .back { opacity: 1; }
  .waiting { position: absolute; inset: 0; background: var(--panel); }
  .label {
    position: absolute;
    inset: auto 0 0;
    padding: 16px 8px 5px;
    background: linear-gradient(transparent, var(--raised) 65%);
    color: var(--text-2);
    font-size: var(--text-xs);
    font-weight: 500;
    opacity: 0;
    pointer-events: none;
    text-align: center;
    transition: opacity 100ms ease-out;
  }
  .tile:hover .label { opacity: 1; }
  .tick { position: absolute; top: 7px; right: 7px; display: grid; place-items: center; width: 22px; height: 22px; background: var(--accent); clip-path: var(--stepped); color: var(--on-accent); pointer-events: none; }
  .edit { position: absolute; top: 7px; left: 7px; display: grid; place-items: center; width: 28px; height: 28px; background: var(--control); clip-path: var(--stepped); color: var(--text-2); opacity: 0; }
  .tile:hover .edit, .edit:focus-visible { opacity: 1; }
  .edit:hover { color: var(--text); }
  .add { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; width: 100%; background: transparent; box-shadow: inset 0 0 0 2px var(--line-strong); clip-path: none; color: var(--text-2); }
  .add:hover:not(:disabled) { background: var(--panel); color: var(--text); }
  .add-label { font-weight: 600; }

  .notice {
    position: sticky;
    bottom: 0;
    z-index: 2;
    justify-content: space-between;
    gap: 12px;
    margin-top: 14px;
  }
  strong { font-weight: 600; }

  .arms { display: flex; gap: 18px; padding-top: 8px; }
  .check { display: flex; align-items: center; gap: 7px; cursor: pointer; }
  .buttons { display: flex; gap: 8px; padding-top: 14px; }
  .spacer { flex: 1; }
</style>

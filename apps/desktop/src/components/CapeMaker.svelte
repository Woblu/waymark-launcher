<script lang="ts">
  import { Check, Download, ImagePlus, X } from '@lucide/svelte';
  import { api, inTauri, pickSavePath } from '../lib/api';
  import type { MadeCape, SkinProfile } from '../lib/api';
  import { CAPE_SCALES, drawCape, fitCrop, wholeCrop } from '../lib/cape';
  import type { Crop } from '../lib/cape';
  import { app } from '../lib/state.svelte';

  let {
    profile,
    onpreview,
    onmade,
    onclose,
  }: {
    /** What the player wears, from Mojang, or null when that isn't known. */
    profile: SkinProfile | null;
    /** Hears about the cape after every change, and null when there's none. */
    onpreview: (cape: HTMLCanvasElement | null) => void;
    /** Hears about a cape that was kept, and what the player wears after, or null if it couldn't be worn. */
    onmade: (cape: MadeCape, profile: SkinProfile | null) => void;
    onclose: () => void;
  } = $props();

  /** The real cape a made one takes the place of, as the core picks it. */
  const base = $derived(profile?.capes.find((each) => each.state === 'ACTIVE') ?? profile?.capes[0]);
  /** Why the cape can't be worn, or what happens when it is. */
  const wearHint = $derived.by(() => {
    if (!app.activeAccount) return 'Sign in to wear it in game.';
    if (!profile) return 'Could not reach Minecraft to see which capes you have.';
    if (!base) return 'To wear it in game, you need a cape from Mojang for it to take the place of.';
    const alias = base.alias ?? 'Mojang';
    if (base.state === 'ACTIVE') return `In game, it takes the place of your ${alias} cape, from the next time you open Minecraft. Only you see it. Other players see the real one, and on your screen, anyone else wearing that cape has your picture too.`;
    return `Wearing it also puts on your ${alias} cape, which other players see. In game, you see your picture in its place, from the next time you open Minecraft. On your screen, anyone else wearing that cape has your picture too.`;
  });

  /** How far the slider zooms in, as the frame's share of the biggest one. */
  const MOST_ZOOM = 8;
  /** The picture is shown at most this tall. */
  const SHOWN_HEIGHT = 300;

  const cape = document.createElement('canvas');
  let input: HTMLInputElement;
  let cropBox = $state<HTMLDivElement>();
  let shown = $state<HTMLCanvasElement>();

  let picture = $state.raw<ImageBitmap | null>(null);
  let name = $state('');
  let crop = $state<Crop>({ x: 0, y: 0, width: 1, height: 1 });
  let zoom = $state(1);
  let colorMode = $state<'match' | 'chosen'>('match');
  /** While matching, this follows the picture, so choosing starts from it. */
  let color = $state('#808080');
  let onElytra = $state(true);
  let scale = $state(CAPE_SCALES[CAPE_SCALES.length - 1]);
  let busy = $state(false);

  let dragging: { x: number; y: number; from: Crop } | null = null;

  async function picked() {
    const file = input.files?.[0];
    input.value = '';
    if (!file) return;
    try {
      const bitmap = await createImageBitmap(file);
      picture?.close();
      picture = bitmap;
      name = file.name.replace(/\.[^.]+$/, '').replace(/[-_]+/g, ' ').trim();
      crop = wholeCrop(bitmap);
      zoom = 1;
    } catch (error) {
      app.fail(error, 'Could not open that picture');
    }
  }

  function moveTo(middleX: number, middleY: number, width = crop.width) {
    if (picture) crop = fitCrop(picture, middleX, middleY, width);
  }

  function zoomTo(value: number) {
    if (!picture) return;
    zoom = Math.min(Math.max(value, 1), MOST_ZOOM);
    moveTo(crop.x + crop.width / 2, crop.y + crop.height / 2, wholeCrop(picture).width / zoom);
  }

  function startDrag(event: PointerEvent) {
    if (!picture || !cropBox || event.button !== 0) return;
    cropBox.setPointerCapture(event.pointerId);
    dragging = { x: event.clientX, y: event.clientY, from: crop };
  }

  function drag(event: PointerEvent) {
    if (!dragging || !picture || !cropBox) return;
    const perPixel = picture.width / cropBox.clientWidth;
    const { from } = dragging;
    moveTo(from.x + from.width / 2 + (event.clientX - dragging.x) * perPixel, from.y + from.height / 2 + (event.clientY - dragging.y) * perPixel);
  }

  function wheel(event: WheelEvent) {
    if (!picture) return;
    event.preventDefault();
    zoomTo(zoom * Math.exp(-event.deltaY * 0.002));
  }

  function nudge(event: KeyboardEvent) {
    if (!picture) return;
    const step = Math.max(1, Math.min(picture.width, picture.height) / 50);
    const moves: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    const move = moves[event.key];
    if (move) moveTo(crop.x + crop.width / 2 + move[0], crop.y + crop.height / 2 + move[1]);
    else if (event.key === '+' || event.key === '=') zoomTo(zoom * 1.25);
    else if (event.key === '-') zoomTo(zoom / 1.25);
    else return;
    event.preventDefault();
  }

  // The picture as the player sees it while cropping, drawn once per picture.
  $effect(() => {
    if (!picture || !shown) return;
    const fit = Math.min(1, 640 / Math.max(picture.width, picture.height));
    shown.width = Math.max(1, Math.round(picture.width * fit));
    shown.height = Math.max(1, Math.round(picture.height * fit));
    shown.getContext('2d')!.drawImage(picture, 0, 0, shown.width, shown.height);
  });

  const design = (from: ImageBitmap) => ({ picture: from, crop: { ...crop }, color: colorMode === 'chosen' ? color : null, onElytra, scale });

  /** The finished cape, drawn now, as the base64 PNG the app takes. */
  function finished(from: ImageBitmap): string {
    drawCape(cape, design(from));
    return cape.toDataURL('image/png').slice('data:image/png;base64,'.length);
  }

  // Redrawn at most once a frame, however fast the slider moves.
  let frame = 0;
  $effect(() => {
    if (!picture) return;
    const next = design(picture);
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const used = drawCape(cape, next);
      if (colorMode === 'match') color = used;
      onpreview(cape);
    });
  });

  $effect(() => () => {
    cancelAnimationFrame(frame);
    picture?.close();
    onpreview(null);
  });

  async function save() {
    if (!picture) return;
    const target = await pickSavePath(`${name.trim() || 'Cape'} cape.png`, 'PNG', 'png');
    if (!target) return;
    busy = true;
    try {
      await api.saveCape(target, finished(picture));
      app.toast('good', 'Cape saved', target);
    } catch (error) {
      app.fail(error, 'Could not save the cape');
    }
    busy = false;
  }

  async function saveAndWear() {
    if (!picture || !base) return;
    busy = true;
    let made: MadeCape | null = null;
    try {
      made = await api.makeCape(name, finished(picture));
      const next = await api.wearMadeCape(made.id);
      onmade(made, next);
      app.toast('good', `Now wearing ${made.name}`, 'It shows the next time you open Minecraft.');
    } catch (error) {
      if (made) onmade(made, null);
      app.fail(error, 'Could not wear the cape');
    }
    busy = false;
  }
</script>

<section class="maker">
  <div class="head">
    <h2>Make a cape</h2>
    <button class="btn quiet" onclick={onclose}><X size={15} /> Close</button>
  </div>
  <p class="muted intro">Turns a picture into a cape. Wear it in game in place of a cape you have from Mojang, and switch back whenever you like from the cape list.</p>

  {#if !picture}
    <button class="choose" onclick={() => input.click()}>
      <ImagePlus size={24} />
      <span class="choose-label">Choose a picture</span>
      <span class="faint small">PNG, JPEG, WebP or GIF</span>
    </button>
  {:else}
    <div class="crop-area">
      <!-- Dragging anywhere moves the frame. From the keyboard, the frame itself takes the arrow keys. -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="crop"
        bind:this={cropBox}
        style:aspect-ratio="{picture.width} / {picture.height}"
        style:width="min(100%, {(SHOWN_HEIGHT * picture.width) / picture.height}px)"
        onpointerdown={startDrag}
        onpointermove={drag}
        onpointerup={() => (dragging = null)}
        onpointercancel={() => (dragging = null)}
        onwheel={wheel}
      >
        <canvas bind:this={shown}></canvas>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <div
          class="frame"
          role="group"
          tabindex="0"
          aria-label="The part of the picture on the cape. Arrow keys move it, plus and minus zoom."
          style:left="{(crop.x / picture.width) * 100}%"
          style:top="{(crop.y / picture.height) * 100}%"
          style:width="{(crop.width / picture.width) * 100}%"
          style:height="{(crop.height / picture.height) * 100}%"
          onkeydown={nudge}
        ></div>
      </div>
      <p class="faint small">Drag the frame to choose what goes on the cape.</p>
    </div>

    <div class="group">
      <div class="form-row">
        <label for="cape-name">Name</label>
        <input id="cape-name" class="field" maxlength="64" bind:value={name} />
      </div>
      <div class="form-row">
        <label for="cape-zoom">Zoom</label>
        <input id="cape-zoom" type="range" min="1" max={MOST_ZOOM} step="0.01" value={zoom} oninput={(event) => zoomTo(Number(event.currentTarget.value))} />
      </div>
      <div class="form-row">
        <span class="form-label">Edges and inside</span>
        <div class="choices">
          <label class="check"><input type="radio" name="cape-color" value="match" bind:group={colorMode} /> Match the picture</label>
          <div class="check">
            <label class="check"><input type="radio" name="cape-color" value="chosen" bind:group={colorMode} /> This color</label>
            <input type="color" class="swatch" aria-label="Color for the edges and inside" bind:value={color} oninput={() => (colorMode = 'chosen')} />
          </div>
        </div>
      </div>
      <div class="form-row">
        <span class="form-label">Elytra</span>
        <label class="check"><input type="checkbox" bind:checked={onElytra} /> Put the picture on the elytra too</label>
      </div>
      <div class="form-row">
        <label for="cape-size">Size</label>
        <div>
          <select id="cape-size" class="field" bind:value={scale}>
            {#each CAPE_SCALES as each (each)}<option value={each}>{64 * each} × {32 * each}</option>{/each}
          </select>
          <p class="form-hint">Minecraft's own capes are 64 × 32. Larger ones look sharper, and the game shows them at full size.</p>
        </div>
      </div>
    </div>

    <div class="buttons">
      <button class="btn" onclick={() => input.click()}><ImagePlus size={15} /> Choose another picture</button>
      <span class="spacer"></span>
      <button class="btn" disabled={!inTauri || busy} onclick={save}><Download size={15} /> Save as PNG</button>
      <button class="btn primary" disabled={!inTauri || busy || !base} onclick={saveAndWear}><Check size={15} /> Save and wear</button>
    </div>
    <p class="form-hint in-game">{wearHint}</p>
  {/if}

  <input bind:this={input} type="file" accept="image/png,image/jpeg,image/webp,image/gif" hidden onchange={picked} />
</section>

<style>
  .maker { padding-top: 6px; }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 6px; }
  h2 { font-size: var(--text-l); }
  .intro { max-width: 62ch; margin-bottom: 16px; }

  .choose { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4px; width: 100%; height: 180px; box-shadow: inset 0 0 0 2px var(--line-strong); color: var(--text-2); }
  .choose:hover { background: var(--panel); color: var(--text); }
  .choose-label { font-weight: 600; }

  .crop-area { display: flex; flex-direction: column; align-items: center; gap: 8px; margin-bottom: 16px; }
  .crop { position: relative; max-width: 100%; overflow: hidden; background: var(--panel); cursor: grab; touch-action: none; }
  .crop:active { cursor: grabbing; }
  .crop canvas { display: block; width: 100%; height: 100%; }
  /* Everything outside the frame is dimmed by the frame's own shadow. */
  .frame { position: absolute; box-shadow: 0 0 0 2px var(--accent), 0 0 0 9999px rgb(0 0 0 / 55%); }
  .frame:focus-visible { outline: 2px solid var(--text); outline-offset: 2px; }

  .choices { display: flex; flex-wrap: wrap; gap: 10px 22px; padding-top: 8px; }
  .check { display: flex; align-items: center; gap: 7px; cursor: pointer; }
  .form-row > .check { padding-top: 8px; }
  .swatch { width: 34px; height: 22px; padding: 0; border: 0; background: none; cursor: pointer; }
  .swatch::-webkit-color-swatch-wrapper { padding: 0; }
  .swatch::-webkit-color-swatch { border: 1px solid var(--line-strong); }
  select.field { max-width: 220px; }

  .buttons { display: flex; flex-wrap: wrap; gap: 8px; padding-top: 14px; }
  .spacer { flex: 1; }
  .in-game { max-width: 62ch; margin-top: 10px; }
</style>

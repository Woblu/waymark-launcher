<script lang="ts">
  import { onMount } from 'svelte';
  import { MoveHorizontal } from '@lucide/svelte';
  import type { SkinViewer } from 'skinview3d';
  import type { SkinVariant } from '../lib/api';
  import { modelName } from '../lib/skin-render';
  import Art from './Art.svelte';

  let { skin, variant, cape, name }: { skin: string; variant: SkinVariant; cape?: string | null; name: string } = $props();

  const START_TURN = Math.PI / 8;
  /** With these the model fills about 55% of the stage, which .tag and .floor line up with. */
  const FOV = 35;
  const ZOOM = 0.57;

  let stage: HTMLDivElement;
  let canvas: HTMLCanvasElement;
  let viewer = $state<SkinViewer | undefined>();
  let failed = $state(false);

  onMount(() => {
    let gone = false;
    let created: SkinViewer | undefined;
    const sizes = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      if (width && height) created?.setSize(width, height);
    });

    (async () => {
      const { IdleAnimation, SkinViewer } = await import('skinview3d');
      if (gone) return;
      created = new SkinViewer({ canvas, width: stage.clientWidth, height: stage.clientHeight, fov: FOV, zoom: ZOOM });
      created.controls.enableZoom = false;
      created.controls.enablePan = false;
      // Only ever turns the model around, never tips it over.
      created.controls.minPolarAngle = Math.PI / 2;
      created.controls.maxPolarAngle = Math.PI / 2;
      created.playerWrapper.rotation.y = START_TURN;
      if (!matchMedia('(prefers-reduced-motion: reduce)').matches) created.animation = new IdleAnimation();
      sizes.observe(stage);
      viewer = created;
    })().catch(() => (failed = true));

    return () => {
      gone = true;
      sizes.disconnect();
      created?.dispose();
    };
  });

  $effect(() => {
    const model = viewer;
    if (!model) return;
    Promise.resolve(model.loadSkin(skin, { model: modelName(variant) })).catch(() => {});
  });

  $effect(() => {
    const model = viewer;
    if (!model) return;
    if (cape) Promise.resolve(model.loadCape(cape)).catch(() => {});
    else model.resetCape();
  });
</script>

<div class="stage" bind:this={stage}>
  <div class="scenery"><Art seed={name} width={230} height={250} /></div>
  <span class="floor"></span>
  <canvas bind:this={canvas} aria-label="A 3D model of {name}'s skin. Drag to rotate."></canvas>
  <div class="over"><span class="tag" class:long={name.length > 12}>{name}</span></div>
  <p class="hint"><MoveHorizontal size={16} /> Drag to rotate</p>
  {#if failed}<p class="muted problem">This computer can't show the 3D model.</p>{/if}
</div>

<style>
  .stage { --step: 3px; position: relative; flex: 1; min-height: 0; overflow: hidden; background: var(--panel); clip-path: var(--stepped); cursor: grab; }
  .scenery { position: absolute; inset: 0; }
  .stage:active { cursor: grabbing; }
  canvas { position: absolute; inset: 0; display: block; transform: translateY(-4%); }

  /* In front of the model and clear of it, so it stays sharp and always faces the player. */
  .over {
    position: absolute;
    right: 8px;
    bottom: calc(82% + 14px);
    left: 8px;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }
  /* Monocraft is drawn on a grid of 9 to the em, so whole multiples land on real pixels. */
  .tag {
    overflow: hidden;
    padding: 2px 9px 3px;
    background: rgb(0 0 0 / 38%);
    color: #fff;
    font: 400 27px / 1.25 'Monocraft', monospace;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag.long { font-size: 18px; }

  .floor {
    position: absolute;
    top: 75%;
    left: 50%;
    width: 280px;
    height: 56px;
    border-radius: 50%;
    background: radial-gradient(closest-side, rgb(0 0 0 / 42%), rgb(0 0 0 / 18%) 55%, transparent);
    pointer-events: none;
    transform: translate(-50%, -50%);
  }
  .hint {
    position: absolute;
    top: calc(75% + 44px);
    right: 0;
    left: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: #c8cfd8;
    font-weight: 500;
    pointer-events: none;
  }
  .problem { position: absolute; inset: 0; display: grid; place-items: center; text-align: center; }
</style>

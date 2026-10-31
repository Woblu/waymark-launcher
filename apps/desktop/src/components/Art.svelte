<script lang="ts">
  import { artUrl } from '../lib/art';

  let { seed, width = 520, height = 92 }: { seed: string; width?: number; height?: number } = $props();

  const src = $derived(artUrl(seed, width, height));
</script>

<img {src} {width} {height} alt="" draggable="false" />

<style>
  img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: 76% 100%;
    image-rendering: pixelated;
  }
</style>

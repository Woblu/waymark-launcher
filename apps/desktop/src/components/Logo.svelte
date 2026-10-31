<script lang="ts">
  let { size = 16 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" shape-rendering="crispEdges" aria-hidden="true">
  <rect x="1" y="12" width="14" height="3" fill="#6d727c" />
  <rect x="3" y="8" width="10" height="3" fill="#979ba4" />
  <rect x="5" y="4" width="6" height="3" fill="#c8cad0" />
  <rect x="7" y="1" width="2" height="2" fill="#ffc36e" />
</svg>

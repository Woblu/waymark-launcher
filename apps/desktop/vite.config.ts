import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // Nothing is inlined, because the release security policy refuses data: fonts.
  build: { target: 'es2022', cssMinify: true, reportCompressedSize: false, assetsInlineLimit: 0 },
});

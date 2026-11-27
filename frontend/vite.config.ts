import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// The Rust binary embeds the build output under fixed names (see build.rs and src/admin/assets.rs).
export default defineConfig({
  plugins: [svelte()],
  // Development only: `vite` serves the admin UI on 127.0.0.1:5291 and forwards the API and icons to a
  // local ShortCircuit admin listener (SHORTCIRCUIT_DEV_API, default http://127.0.0.1:18281) whose
  // ADMIN_ORIGIN is http://localhost:5291. Not used by `vite build`.
  server: {
    host: '127.0.0.1',
    port: 5291,
    strictPort: true,
    proxy: Object.fromEntries(
      ['/api', '/favicon.svg', '/favicon.png'].map((path) => [path, { target: process.env.SHORTCIRCUIT_DEV_API ?? 'http://127.0.0.1:18281' }]),
    ),
  },
  build: {
    cssCodeSplit: false,
    modulePreload: false,
    assetsInlineLimit: 0,
    rolldownOptions: {
      output: {
        entryFileNames: 'assets/admin.js',
        assetFileNames: (asset) =>
          asset.names.some((name) => name.endsWith('.woff2')) ? 'assets/[name][extname]' : 'assets/admin[extname]',
      },
    },
  },
});

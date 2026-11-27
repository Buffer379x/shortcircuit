import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// The Rust binary embeds the build output under fixed names (see build.rs and src/admin/assets.rs).
export default defineConfig({
  plugins: [svelte()],
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

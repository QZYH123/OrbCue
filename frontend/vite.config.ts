import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
  plugins: [
    svelte(),
    {
      name: 'tauri-no-crossorigin',
      transformIndexHtml(html) {
        return html.replaceAll(' crossorigin', '');
      },
    },
  ],
  // Relative asset URLs so bundled theme CSS loads under Tauri's custom protocol.
  base: './',
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});


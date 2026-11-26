import { defineConfig } from 'vite'

export default defineConfig({
  // GitHub Pages serves the site under /<repo>/; the Pages workflow sets FV_BASE.
  base: process.env.FV_BASE ?? '/',
  build: {
    target: 'es2022',
    rollupOptions: { input: { main: 'index.html', determinism: 'determinism.html' } },
  },
})

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  base: './',
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          // jsQR is only reached through a dynamic import in the eSIM tab, and
          // folding it into `vendor` would put 130 KB of QR decoder in the
          // eagerly-loaded bundle for every page that never scans anything.
          if (id.includes('node_modules/jsqr')) return
          // Same reasoning for the Tauri HTTP plugin: only the desktop build
          // ever imports it, and folding it into `vendor` would ship it to
          // every browser loading the dashboard off the router.
          if (id.includes('node_modules/@tauri-apps')) return
          if (id.includes('node_modules')) return 'vendor'
        },
      },
    },
  },
})

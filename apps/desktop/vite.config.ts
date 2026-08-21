import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Vite config follows the Tauri convention: fixed dev port, strict-port,
// and awareness of the TAURI_DEV_HOST env var used for mobile dev.
// https://v2.tauri.app/start/frontend/vite/
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
})

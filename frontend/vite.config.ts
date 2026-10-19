import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    // 5174/8081 keep this clear of the tenant platform's dev servers
    // (5173/8080) so both can run at once.
    port: 5174,
    proxy: {
      // Dev-only: in production the built SPA is served by the same Axum
      // process that owns `/api/v1`, so requests are same-origin.
      '/api/v1': {
        target: 'http://localhost:8081',
        changeOrigin: true,
      },
    },
  },
})

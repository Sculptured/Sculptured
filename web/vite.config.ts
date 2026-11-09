/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: process.env.SCULPTURED_API ?? 'http://127.0.0.1:4410',
        ws: false,
        // controld's guard accepts only its own Host and Origin. Rewrite the Host and
        // drop the dev server's Origin, which the guard would reject.
        changeOrigin: true,
        configure: (proxy) => proxy.on('proxyReq', (req) => req.removeHeader('origin')),
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
  },
});

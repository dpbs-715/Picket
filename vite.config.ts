import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

const rootDir = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [react()],
  server: { port: 1420, strictPort: true, host: '127.0.0.1' },
  clearScreen: false,
  build: {
    rollupOptions: {
      input: {
        main: resolve(rootDir, 'index.html'),
        splash: resolve(rootDir, 'splash.html'),
        tray: resolve(rootDir, 'tray.html'),
      },
    },
  },
  test: {
    include: ['src/**/*.test.ts', 'agent/**/*.test.ts'],
    testTimeout: 15_000,
  },
});

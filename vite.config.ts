import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react()],
  server: { port: 1420, strictPort: true, host: '127.0.0.1' },
  clearScreen: false,
  test: {
    include: ['src/**/*.test.ts', 'agent/**/*.test.ts'],
    testTimeout: 15_000,
  },
});

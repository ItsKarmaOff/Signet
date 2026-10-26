import { resolve } from 'node:path'
import { defineConfig } from 'vitest/config'

// Unit tests: `*.test.ts(x)` files placed next to the code they test. End-to-end tests are in tests/e2e (Playwright).
export default defineConfig({
  resolve: { alias: { '@shared': resolve(__dirname, 'src/shared') } },
  test: {
    include: ['src/**/*.test.{ts,tsx}'],
    environment: 'node'
  }
})

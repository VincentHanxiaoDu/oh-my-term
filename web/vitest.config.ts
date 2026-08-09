import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    // `e2e/` belongs to Playwright: those specs need a real browser and a
    // running instance, and vitest collecting them turns a green run into a
    // failure that says nothing about the code.
    include: ['test/**/*.test.ts'],
  },
})

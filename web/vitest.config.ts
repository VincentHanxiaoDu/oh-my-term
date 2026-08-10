import { fileURLToPath } from 'node:url'

import { defineConfig } from 'vitest/config'

export default defineConfig({
  // Pinned to this directory rather than left to the working directory. The
  // include pattern below is relative to the root, so an invocation from the
  // repository root would otherwise reach into `.research/` — the gitignored
  // checkouts of studied code, which `docs/architecture/14-licensing.md` §5
  // says are never vendored and never built. Running their test suite is a
  // milder version of the same mistake, and it fails for reasons that have
  // nothing to do with omt.
  root: fileURLToPath(new URL('.', import.meta.url)),
  test: {
    // `e2e/` belongs to Playwright: those specs need a real browser and a
    // running instance, and vitest collecting them turns a green run into a
    // failure that says nothing about the code.
    include: ['test/**/*.test.ts'],
    exclude: ['**/node_modules/**', '**/.research/**'],
  },
})

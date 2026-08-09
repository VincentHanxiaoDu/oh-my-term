import { defineConfig } from '@playwright/test'

/**
 * These run against a real `omt web`, not a mock.
 *
 * A mocked server would have passed every previous round too: the failures
 * were in what the browser does with a focused element and what the client
 * does with an unrelated store update, and neither involves the server being
 * wrong.
 */
export default defineConfig({
  testDir: './e2e',
  // The invite is exchanged once, before anything runs. It is single-exchange
  // by design, so handing the same one to every spec would join on the first
  // test and be refused for the rest.
  globalSetup: './e2e/join-once.ts',
  // A phone is one screen. Running these in parallel against one daemon has
  // them fighting over the same sessions.
  workers: 1,
  use: {
    baseURL: process.env.OMT_URL ?? 'http://127.0.0.1:7788',
    trace: 'retain-on-failure',
  },
})

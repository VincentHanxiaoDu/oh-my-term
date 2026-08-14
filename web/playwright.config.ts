import { defineConfig, devices } from '@playwright/test'

/**
 * These run against a real `omt web`, not a mock.
 *
 * A mocked server would have passed every previous round too: the failures were
 * in what the browser does with a focused element and what the client does with
 * an unrelated store update, and neither involves the server being wrong.
 *
 * Start the instance with a **state file of its own**:
 *
 *   OMT_STATE_FILE=$(mktemp -d)/state.json omt serve --web 127.0.0.1:7788
 *   OMT_TOKEN=<the printed invite> npx playwright test
 *
 * Not optional. omt restores what was open when it last stopped, and a
 * restored session is an orphan with no process behind it. On a machine that
 * has run omt before, the roster's first row is a corpse — and these tests
 * open the first row. That hid a real product gap (nothing offers to restart
 * or close an orphan) for as long as the suite was green, and it made the
 * empty-roster path unreachable, so six call sites went on tapping a button
 * that had been removed.
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
  // Two devices, because one was not enough. Everything ran at an iPhone 13 and
  // passed while, on Android, a long instance name pushed the action button
  // onto a third line where nobody taps. A different width and a different font
  // scale find a class of layout bug a single viewport cannot.
  projects: [
    { name: 'iphone', use: { ...devices['iPhone 13'] } },
    { name: 'android', use: { ...devices['Pixel 7'] } },
  ],
})

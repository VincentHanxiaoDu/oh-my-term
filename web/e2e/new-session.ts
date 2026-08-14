/**
 * Create a session, the way the client actually offers it.
 *
 * There is no "New session" button. The roster's `+` opens the **project
 * picker**, because with three workspaces creating a session in whichever one
 * happened to be first picked one silently and the person who pressed it had
 * no way to know which.
 *
 * Six call sites across two files were still tapping a button that had not
 * existed for some time, and every one of them was green — because a machine
 * that has ever run omt has leftover sessions, so the branch that creates one
 * was never reached. It is reached on a clean state file, which is what CI
 * has, and it failed there immediately. One helper, so the next rename breaks
 * in one place instead of six.
 */
import { expect, type Page } from '@playwright/test'

/**
 * The roster's first row that is a plain shell.
 *
 * Never an agent session: a session with an agent in it opens on its
 * conversation rather than its terminal — correctly — and a test about typing
 * into a terminal then finds no terminal. The suite shares one instance and
 * runs in order, so an earlier test's attached agent is sitting in the roster
 * by the time a later one taps "the first row".
 */
export function firstShellRow(page: Page) {
  return page.locator('.body button.tap').filter({ hasNotText: 'codex' }).first()
}

/** Create a session and land on its terminal. */
export async function createASession(page: Page): Promise<void> {
  await page.getByRole('button', { name: /Open a project/ }).tap()
  // A workspace row, not one of its sub-rows: Files and Changes are nested
  // under each workspace and neither of them is a session.
  await page.locator('.body button.tap:not(.sub)').first().tap()
  await expect(page.locator('.terminal-body')).toBeVisible({ timeout: 15_000 })
}

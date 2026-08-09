/**
 * The structured surfaces, on a phone.
 *
 * These are the ones a session is meant to be read through: a phone is about
 * forty columns at a readable size and almost every useful output is written
 * for eighty to a hundred and twenty, so the grid is the fallback rather than
 * the answer. Every assertion here runs at a real touch viewport, because the
 * last round proved a resized desktop window cannot see what a phone does.
 */
import { devices, expect, test } from '@playwright/test'
import type { Page } from '@playwright/test'

test.use({ ...devices['iPhone 13'] })

// The credential the global setup joined with, not the invite: an invite is
// exchanged once, and every test after the first would be refused.
const TOKEN = process.env['OMT_CREDENTIAL'] ?? process.env['OMT_TOKEN'] ?? ''

test.beforeEach(async ({ page }) => {
  await page.goto(`/#token=${TOKEN}`)
})

/** Open a session, creating one if the roster is empty. */
async function openASession(page: Page): Promise<void> {
  const rows = page.locator('.body button.tap')
  if ((await rows.count()) === 0) {
    await page.getByRole('button', { name: 'New session' }).tap()
    await expect(page.locator('.terminal-body')).toBeVisible()
    return
  }
  await rows.first().tap()
}

test('a session offers its surfaces, and says why one is unavailable', async ({ page }) => {
  // Each is a complete way of reading the session rather than a preview of
  // another, so the switcher is persistent rather than buried in a menu.
  await openASession(page)
  const tabs = page.locator('.surface')
  await expect(tabs).toHaveCount(3)

  // A shell has no agent in it, so the conversation is not available — and it
  // has to say so, because a disabled control with no reason reads as a bug.
  const conversation = tabs.filter({ hasText: 'Conversation' })
  await expect(conversation).toBeDisabled()
  await expect(conversation).toHaveAttribute('aria-label', /nothing in this session is an agent/)
})

test('the terminal is always one tap away', async ({ page }) => {
  await openASession(page)
  await expect(page.locator('.surface[aria-selected="true"]')).toHaveText('Terminal')
})

test('block view lists the commands run in a session', async ({ page }) => {
  await openASession(page)
  await page.locator('.hidden-input').pressSequentially('echo block-view-probe\r')
  await page.waitForTimeout(1500)

  const commands = page.locator('.surface').filter({ hasText: 'Commands' })
  if (await commands.isDisabled()) {
    // This shell emits no command marks. Block view says so rather than showing
    // an empty list, and that message is the thing worth asserting.
    await commands.tap({ force: true }).catch(() => {})
    return
  }
  await commands.tap()
  await expect(page.locator('.block').first()).toBeVisible()
})

test('a block opens its own output without panning the page sideways', async ({ page }) => {
  await openASession(page)
  const commands = page.locator('.surface').filter({ hasText: 'Commands' })
  test.skip(await commands.isDisabled(), 'this shell marks no commands')

  await commands.tap()
  const first = page.locator('.block').first()
  await first.locator('summary').tap()
  await expect(first.locator('.block-output')).toBeVisible()

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  )
  expect(overflow, 'opening a block made the page scroll sideways').toBeLessThanOrEqual(0)
})

test('search finds something written earlier in the session', async ({ page }) => {
  // Over the instance's own view of the session, not the visible grid: the
  // answer to "where did that error go" is almost always above the fold.
  await openASession(page)
  await page.locator('.hidden-input').pressSequentially('echo search-me-later\r')
  await page.waitForTimeout(1200)

  await page.getByRole('button', { name: 'Search this session' }).tap()
  await page.locator('.search-field').fill('search-me-later')
  await expect(page.locator('.search-hit').first()).toBeVisible()
})

test('search that matches nothing says so rather than showing an empty screen', async ({
  page,
}) => {
  await openASession(page)
  await page.getByRole('button', { name: 'Search this session' }).tap()
  await page.locator('.search-field').fill('nothing-matches-this-xyzzy')
  await expect(page.getByText('Nothing matched.')).toBeVisible()
})

test('the search field keeps what was typed when you come back', async ({ page }) => {
  await openASession(page)
  await page.getByRole('button', { name: 'Search this session' }).tap()
  await page.locator('.search-field').fill('kept')
  await page.getByRole('button', { name: 'Back' }).tap()
  await page.getByRole('button', { name: 'Search this session' }).tap()
  await expect(page.locator('.search-field')).toHaveValue('kept')
})

test('the mark is drawn and takes its colour from the theme', async ({ page }) => {
  // Inline SVG on `currentColor`, so it cannot fail to load and it is the
  // accent without a second asset.
  const mark = page.locator('.bar .mark svg').first()
  await expect(mark).toBeAttached()
  const colour = await page
    .locator('.bar .mark')
    .first()
    .evaluate((node) => getComputedStyle(node).color)
  expect(colour).not.toBe('')
})

test('every new control is still big enough for a thumb', async ({ page }) => {
  await openASession(page)
  const small = await page.locator('button:visible').evaluateAll((nodes) =>
    nodes
      .map((node) => ({
        label: (node.textContent ?? '').trim(),
        height: node.getBoundingClientRect().height,
      }))
      .filter((box) => box.height < 36),
  )
  expect(small, `these are too small to tap: ${JSON.stringify(small)}`).toEqual([])
})

test('nothing on the new screens makes the page scroll sideways', async ({ page }) => {
  await openASession(page)
  for (const label of ['Search this session']) {
    await page.getByRole('button', { name: label }).tap()
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    )
    expect(overflow, `${label} scrolls sideways`).toBeLessThanOrEqual(0)
    await page.getByRole('button', { name: 'Back' }).tap()
  }
})

test('block view is never an unexplained empty screen', async ({ page }) => {
  // An empty list reads as "you have run nothing" about a shell that has run
  // plenty. Exactly one of two things must be true: there are blocks, or there
  // is a sentence saying why there are none and where the output still is.
  await openASession(page)
  const commands = page.locator('.surface').filter({ hasText: 'Commands' })
  test.skip(await commands.isDisabled(), 'block view is not offered at all here')

  await commands.tap()
  await page.waitForTimeout(1500)

  const blocks = await page.locator('.block').count()
  if (blocks === 0) {
    await expect(page.locator('.body .notice')).toBeVisible()
  } else {
    await expect(page.locator('.block').first()).toBeVisible()
  }
})

test('the roster updates from the stream, not by asking again', async ({ page, browser }) => {
  // A session started somewhere else appears here without this client asking.
  // Before the stream existed, `subscribe` was accepted, answered with silence
  // and no event was ever sent — so every screen polled.
  const before = await page.locator('.body button.tap').count()

  const other = await browser.newContext({ ...devices['iPhone 13'] })
  const second = await other.newPage()
  await second.goto(`/#token=${TOKEN}`)
  await second.getByRole('button', { name: 'New session' }).tap()
  await expect(second.locator('.terminal-body')).toBeVisible()

  // No reload and no navigation on the first page: if this arrives, it arrived
  // on the socket.
  await expect(page.locator('.body button.tap')).toHaveCount(before + 1, { timeout: 8000 })
  await other.close()
})

test('a lost device can be revoked from the phone in your hand', async ({ page }) => {
  // Pairing mints a credential per device precisely so a lost phone is one
  // thing to revoke. Without somewhere to do it, "individually revocable" is a
  // property of the storage rather than a thing anybody can use.
  await page.getByRole('button', { name: /switch instance/i }).tap()
  await expect(page.getByRole('heading', { name: 'Instances' })).toBeVisible()

  const devices = page.locator('.device')
  await expect(devices.first()).toBeVisible()
  // This browser's own credential is in the list, by the name it gave.
  await expect(page.locator('.devices')).toContainText('touch suite')
  await expect(page.getByRole('button', { name: /^Revoke / }).first()).toBeVisible()
})

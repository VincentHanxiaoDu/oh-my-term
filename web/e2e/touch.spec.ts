/**
 * The client, on a phone.
 *
 * The last round of verification was a desktop browser resized to 390x844, and
 * that is exactly what missed the two failures a user found in a minute: no
 * keyboard, and a spinner that never stopped. A resized desktop window still
 * has a physical keyboard and no soft keyboard at all, so the whole class of
 * bug is invisible to it.
 *
 * So this file runs under a real device profile — `hasTouch`, `isMobile`, a
 * device pixel ratio — and every assertion here is one a desktop viewport
 * cannot make.
 */
import { devices, expect, test } from '@playwright/test'

test.use({ ...devices['iPhone 13'] })

// The credential the global setup joined with, not the invite: an invite is
// exchanged once, and every test after the first would be refused.
const TOKEN = process.env['OMT_CREDENTIAL'] ?? process.env['OMT_TOKEN'] ?? ''

/**
 * Open a session from the roster.
 *
 * By class rather than by accessible name: a name match on /session/i picks up
 * the "New session" button in the bar, which is how an earlier version of this
 * file spent every run creating a shell and then testing the screen it landed
 * on instead of the one it meant to open.
 */
async function openASession(page: import('@playwright/test').Page): Promise<void> {
  const rows = page.locator('.body button.tap')
  if ((await rows.count()) === 0) {
    // Creating one goes straight to its terminal, which is the point of the
    // button — so there is nothing left to tap.
    await page.getByRole('button', { name: 'New session' }).tap()
    await expect(page.locator('.terminal-body')).toBeVisible()
    return
  }
  await rows.first().tap()
}

test.beforeEach(async ({ page }) => {
  // The token travels in the fragment, never the query string: a query string
  // reaches the server access log and the Referer header of anything the page
  // later loads.
  await page.goto(`/#token=${TOKEN}`)
})

test('there is something a soft keyboard can open on', async ({ page }) => {
  // The bug: input was a `keydown` listener on `document`. A phone opens no
  // keyboard for that, because nothing is focused — so the terminal was
  // read-only on the only device this product exists for.
  await openASession(page)

  const input = page.locator('.hidden-input')
  await expect(input).toBeAttached()
  await expect(input).toBeFocused()
})

test('the font is big enough that iOS does not zoom into it', async ({ page }) => {
  // Safari zooms the page when a focused control has text under 16px, and it
  // does not zoom back out — the terminal ends up half off-screen and stays
  // there. The fix is invisible on desktop, so it needs a test that is not.
  await openASession(page)
  const size = await page
    .locator('.hidden-input')
    .evaluate((node) => Number.parseFloat(getComputedStyle(node).fontSize))
  expect(size).toBeGreaterThanOrEqual(16)
})

test('the keyboard is not torn down by every repaint', async ({ page }) => {
  // The terminal repaints every 150ms. Rebuilding the input on each repaint is
  // a keyboard that closes six times a second, an IME composition that can
  // never finish, and dictation that stops mid-sentence — and none of it shows
  // on a desktop, where the keyboard is not attached to the element.
  await openASession(page)
  const input = page.locator('.hidden-input')
  await expect(input).toBeFocused()

  // Several repaints' worth.
  await page.waitForTimeout(1000)
  await expect(input).toBeFocused()

  // And it is still the same element, not a replacement that happens to be
  // focused: a replaced node loses an in-progress composition even when focus
  // is handed back.
  const same = await page.evaluate(() => {
    const node = document.querySelector('.hidden-input')
    ;(node as HTMLElement & { marked?: boolean }).marked = true
    return true
  })
  expect(same).toBe(true)
  await page.waitForTimeout(600)
  const survived = await page.evaluate(
    () =>
      (document.querySelector('.hidden-input') as (HTMLElement & { marked?: boolean }) | null)
        ?.marked === true,
  )
  expect(survived, 'the input element was replaced by a repaint').toBe(true)
})

test('typing reaches the shell and the echo comes back', async ({ page }) => {
  await openASession(page)
  await page.locator('.hidden-input').pressSequentially('echo omt-typed')
  await expect(page.locator('.terminal-body')).toContainText('omt-typed')
})

test('a keystroke that is refused says so instead of doing nothing', async ({ page, browser }) => {
  // Found by typing into a session another browser was driving: nothing
  // happened, and nothing said why. The refusal was recorded and never
  // rendered, so the terminal looked simply broken.
  await page.getByRole('button', { name: 'New session' }).tap()
  await expect(page.locator('.terminal-body')).toBeVisible()

  // A second client, which is what holding the writer token elsewhere means.
  const other = await browser.newContext({ ...devices['iPhone 13'] })
  const second = await other.newPage()
  await second.goto(`/#token=${TOKEN}`)
  const rows = second.locator('.body button.tap')
  await expect(rows.first()).toBeVisible()
  await rows.first().tap()
  await second.locator('.hidden-input').pressSequentially('echo second')

  // Either it was allowed and echoed, or it was refused and said so. What must
  // not happen is neither.
  await expect(
    second.locator('.terminal-body'),
    'a keystroke produced no output and no explanation',
  ).not.toHaveText(/^\s*$/)
  await other.close()
})

test('typing works in a session opened from the roster, not only a new one', async ({ page }) => {
  // A session opened from the roster must be typeable, not only one this
  // client just created — the writer token is claimed on open for that reason.
  // Two sessions, because with only one the stale epoch happens to be the
  // right one and the bug hides completely.
  await page.getByRole('button', { name: 'New session' }).tap()
  await expect(page.locator('.terminal-body')).toBeVisible()
  await page.getByRole('button', { name: 'Back' }).tap()
  await page.getByRole('button', { name: 'New session' }).tap()
  await expect(page.locator('.terminal-body')).toBeVisible()
  await page.getByRole('button', { name: 'Back' }).tap()

  // Now open the *first* one, whose epoch this client is not holding.
  const rows = page.locator('.body button.tap')
  await expect(rows.first()).toBeVisible()
  await rows.first().tap()

  await page.locator('.hidden-input').pressSequentially('echo from-the-roster')
  await expect(page.locator('.terminal-body')).toContainText('from-the-roster')
})

test('the input is emptied after every keystroke', async ({ page }) => {
  // Left un-emptied, the element accumulates: the second keystroke sends both
  // characters again, so "ls" arrives as "lls".
  await openASession(page)
  const input = page.locator('.hidden-input')
  await input.pressSequentially('ab')
  await expect(input).toHaveValue('')
})

test('tapping the terminal brings the keyboard back', async ({ page }) => {
  await openASession(page)
  await page.locator('.hidden-input').blur()
  await page.locator('.terminal-body').tap()
  await expect(page.locator('.hidden-input')).toBeFocused()
})

test('going back and returning shows the terminal, not a spinner', async ({ page }) => {
  // The other reported failure. `store.subscribe(render)` fired on every store
  // change, and a null snapshot meant "loading" — so an unrelated refresh
  // repainted a drawn terminal into a spinner that never resolved.
  await openASession(page)
  await expect(page.locator('.terminal-body')).toBeVisible()

  await page.getByRole('button', { name: 'Back' }).tap()
  await openASession(page)

  // Wait past a refresh tick, because the bug needed one to appear.
  await page.waitForTimeout(1500)
  await expect(page.locator('[role="status"]')).toHaveCount(0)
  await expect(page.locator('.terminal-body')).toBeVisible()
})

test('the grid is sized to the screen, not to the clamp minimum', async ({ page }) => {
  // The cell probe wore the whole `.terminal` class, so it inherited that
  // rule's `display: block` and padding and measured the container instead of a
  // character: 24px wide, the full height of the screen. `fit` clamped to its
  // minimum and a phone showed a 20x4 grid — four lines of a session, with
  // everything else off the edge.
  await openASession(page)
  await page.waitForTimeout(800)

  const size = await page.evaluate(() => {
    const grid = document.querySelector('.terminal-grid')
    const line = grid?.querySelector('.terminal')?.firstElementChild
    return {
      gridWidth: grid?.getBoundingClientRect().width ?? 0,
      lineWidth: line?.getBoundingClientRect().width ?? 0,
    }
  })
  // Nothing drawn may be wider than the box it is drawn in: that is what being
  // clipped at the right-hand edge looks like from here.
  expect(size.lineWidth).toBeLessThanOrEqual(size.gridWidth + 1)
})

test('a full screen of output is shown, not the last few lines', async ({ page }) => {
  // With a four-row grid, `git status` showed one fragment of one line. This
  // asserts the grid is tall enough to be a terminal.
  await openASession(page)
  await page.locator('.hidden-input').pressSequentially('printf "L1\\nL2\\nL3\\nL4\\nL5\\nL6\\nL7\\nL8\\n"\r')
  await page.waitForTimeout(1200)
  const body = page.locator('.terminal-body')
  await expect(body).toContainText('L1')
  await expect(body).toContainText('L8')
})

test('every tap target is big enough for a thumb', async ({ page }) => {
  // 44px is the size below which people miss. A mouse cursor is one pixel, so
  // a desktop run can never fail this.
  const boxes = await page.locator('button:visible').evaluateAll((nodes) =>
    nodes.map((node) => {
      const box = node.getBoundingClientRect()
      return { label: node.textContent ?? '', height: box.height }
    }),
  )
  expect(boxes.length).toBeGreaterThan(0)
  for (const box of boxes) {
    expect(box.height, `"${box.label.trim()}" is ${box.height}px tall`).toBeGreaterThanOrEqual(44)
  }
})

test('nothing makes the page scroll sideways', async ({ page }) => {
  // A 390px-wide viewport finds the fixed widths a 1440px one hides.
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  )
  expect(overflow).toBeLessThanOrEqual(0)
})

test('the terminal has real colours, not undefined variables', async ({ page }) => {
  // An undefined CSS variable renders as nothing rather than as an error, so
  // deleting the palette produced a terminal that looked plausible and had no
  // colour at all. This asserts a resolved value.
  const background = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue('--omt-ansi-1').trim(),
  )
  expect(background).not.toBe('')
})

test('the instance list is reachable and knows where it is connected', async ({ page }) => {
  // Pairing a second device used to mean overwriting the first: one token in
  // one slot, and the replaced one is shown once and cannot be recovered.
  await page.getByRole('button', { name: /switch instance/i }).tap()
  await expect(page.getByRole('heading', { name: 'Instances' })).toBeVisible()
  await expect(page.getByRole('button', { name: /the one in use/i })).toBeVisible()
})

test('forgetting an instance is offered, and is not a swipe', async ({ page }) => {
  // A swipe that destroys a credential which cannot be recovered is a gesture
  // people make by accident on a train.
  await page.getByRole('button', { name: /switch instance/i }).tap()
  await expect(page.getByRole('button', { name: /^Forget / })).toBeVisible()
})

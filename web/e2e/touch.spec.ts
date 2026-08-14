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
import { createASession, firstShellRow } from './new-session.js'

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
  if ((await firstShellRow(page).count()) === 0) {
    await createASession(page)
    return
  }
  await firstShellRow(page).tap()
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

  const input = page.locator('.composer-field')
  await expect(input).toBeAttached()
  await expect(input).toBeFocused()
})

test('the font is big enough that iOS does not zoom into it', async ({ page }) => {
  // Safari zooms the page when a focused control has text under 16px, and it
  // does not zoom back out — the terminal ends up half off-screen and stays
  // there. The fix is invisible on desktop, so it needs a test that is not.
  await openASession(page)
  await expect(page.locator('.composer-field')).toBeVisible()
  const size = await page
    .locator('.composer-field')
    .evaluate((node) => Number.parseFloat(getComputedStyle(node).fontSize))
  expect(size).toBeGreaterThanOrEqual(16)
})

test('the field survives a repaint, keeping focus and a half-typed command', async ({
  page,
}) => {
  // The terminal repaints continuously. Rebuilding the screen each time detaches
  // the field — which blurs it, closes the keyboard, and throws away whatever
  // was being typed.
  await openASession(page)
  const input = page.locator('.composer-field')
  await input.fill('half typed')
  await expect(input).toBeFocused()

  // Several repaints' worth.
  await page.waitForTimeout(1200)
  await expect(input).toBeFocused()
  await expect(input).toHaveValue('half typed')
})

test('a command typed and sent reaches the shell', async ({ page }) => {
  // Drafted in a real field and sent as one line. Putting a paragraph into a
  // pty one keystroke at a time is miserable, and a one-pixel transparent
  // capture field is a shape an input method cannot work with at all.
  await openASession(page)
  await page.locator('.composer-field').fill('echo omt-typed')
  await page.locator('.composer-send').tap()
  await expect(page.locator('.terminal-body')).toContainText('omt-typed')
})

test('a keystroke that is refused says so instead of doing nothing', async ({ page, browser }) => {
  // Found by typing into a session another browser was driving: nothing
  // happened, and nothing said why. The refusal was recorded and never
  // rendered, so the terminal looked simply broken.
  await createASession(page)

  // A second client, which is what holding the writer token elsewhere means.
  const other = await browser.newContext({ ...devices['iPhone 13'] })
  const second = await other.newPage()
  await second.goto(`/#token=${TOKEN}`)
  const row = firstShellRow(second)
  await expect(row).toBeVisible()
  await row.tap()
  await second.locator('.composer-field').fill('echo second')
  await second.locator('.composer-send').tap()

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
  await createASession(page)
  await page.getByRole('button', { name: 'Back' }).tap()
  await createASession(page)
  await page.getByRole('button', { name: 'Back' }).tap()

  // Now open the *first* one, whose epoch this client is not holding.
  const row = firstShellRow(page)
  await expect(row).toBeVisible()
  await row.tap()

  await page.locator('.composer-field').fill('echo from-the-roster')
  await page.locator('.composer-send').tap()
  await expect(page.locator('.terminal-body')).toContainText('from-the-roster')
})

test('the field keeps a draft while typing and empties on send', async ({ page }) => {
  // The opposite of the old rule, and deliberately: a field cleared on every
  // keystroke cannot hold a half-typed command, and cannot hold an input
  // method's candidates either.
  await openASession(page)
  const input = page.locator('.composer-field')
  await input.pressSequentially('echo draft')
  await expect(input).toHaveValue('echo draft')

  await page.locator('.composer-send').tap()
  await expect(input).toHaveValue('')
})

test('Enter sends, and shift-Enter does not', async ({ page }) => {
  // Enter is how a command is run. Shift-Enter is the newline the paragraph
  // this field exists for needs.
  await openASession(page)
  const input = page.locator('.composer-field')
  await input.fill('echo enter-sends')
  await input.press('Enter')
  await expect(page.locator('.terminal-body')).toContainText('enter-sends')
  await expect(input).toHaveValue('')

  await input.pressSequentially('one')
  await input.press('Shift+Enter')
  await expect(input).toHaveValue(/one\n/)
})

test('the field is focused when a session opens, ready to type', async ({ page }) => {
  await openASession(page)
  await expect(page.locator('.composer-field')).toBeFocused()
})

test('every key a phone keyboard lacks is reachable', async ({ page }) => {
  // Eleven of them. In one scrolling row the last four are invisible, and a key
  // nobody can see is a key nobody knows exists.
  await openASession(page)
  const labels = await page.locator('.keybar > button').allTextContents()
  for (const label of ['Esc', 'Tab', 'Ctrl', '↑', '↓', '←', '→', '/', '-', '|', '~']) {
    expect(labels, `${label} is missing from the key bar`).toContain(label)
  }
  // Reachable, not necessarily all on screen at once: the bar is one scrolling
  // row, because wrapped it becomes three rows on a phone with a large font
  // scale. What matters is that scrolling reaches the end — asserted by the
  // test that owns that behaviour.
})

test('the agent keys are reachable, and the bar can be folded away', async ({ page }) => {
  // A second row behind an undiscoverable swipe is a second row nobody finds,
  // and a bar that cannot be hidden eats a forty-column screen.
  await openASession(page)
  await page.getByRole('button', { name: 'Show the agent keys' }).tap()
  const labels = await page.locator('.keybar > button').allTextContents()
  expect(labels).toContain('^C')

  await page.getByRole('button', { name: 'Hide the key bar' }).tap()
  await expect(page.locator('.keybar')).toHaveCount(0)
  await page.getByRole('button', { name: 'Show the key bar' }).tap()
  await expect(page.locator('.keybar')).toBeVisible()
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
  await page.locator('.composer-field').pressSequentially('printf "L1\\nL2\\nL3\\nL4\\nL5\\nL6\\nL7\\nL8\\n"\r')
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

test('a word typed through an input method survives to the field', async ({ page }) => {
  // The failure this exists for: a pinyin keyboard puts `n i h a o` in the
  // field and waits for a character to be chosen, and clearing the field on
  // every `input` event wiped the candidate buffer on the first keystroke — so
  // nothing was ever committed and the terminal stayed empty.
  //
  // Playwright's typing does not go through an input method, so this drives the
  // composition events by hand. It is the only way to reach this path at all.
  await openASession(page)
  const input = page.locator('.composer-field')
  await expect(input).toBeFocused()

  await page.evaluate(() => {
    const field = document.querySelector('.composer-field') as HTMLTextAreaElement
    const compose = (data: string, type: 'compositionstart' | 'compositionupdate') => {
      field.value = data
      field.dispatchEvent(new CompositionEvent(type, { data, bubbles: true }))
      field.dispatchEvent(new InputEvent('input', { data, isComposing: true, bubbles: true }))
    }
    compose('n', 'compositionstart')
    compose('ni', 'compositionupdate')
    compose('nihao', 'compositionupdate')
    field.value = '你好'
    field.dispatchEvent(new CompositionEvent('compositionend', { data: '你好', bubbles: true }))
    field.dispatchEvent(new InputEvent('input', { data: '你好', bubbles: true }))
  })

  // The chosen characters are in the field, and the letters that produced them
  // are not. Nothing has been sent yet — that is what the send button is for,
  // and it is the whole reason a field beats a capture box on a phone.
  await expect(input).toHaveValue('你好')
  await expect(page.locator('.terminal-body')).not.toContainText('nihao')

  await page.locator('.composer-send').tap()
  await expect(page.locator('.terminal-body')).toContainText('你好', { timeout: 5000 })
  await expect(input).toHaveValue('')
})

test('the key bar is one row, and every key can be reached by scrolling', async ({
  page,
}) => {
  // Wrapped, eleven keys become three rows on a phone with a large font scale —
  // half the screen given over to punctuation. One row that scrolls keeps the
  // grid; the order is what makes that safe.
  await openASession(page)
  const bar = page.locator('.keybar')
  const height = await bar.evaluate((node) => node.getBoundingClientRect().height)
  const key = await page
    .locator('.keybar > button')
    .first()
    .evaluate((node) => node.getBoundingClientRect().height)
  expect(height, 'the key bar is more than one row tall').toBeLessThan(key * 1.6)

  // And it really does scroll rather than clipping what does not fit.
  const scrolls = await bar.evaluate((node) => node.scrollWidth > node.clientWidth)
  const labels = await page.locator('.keybar > button').allTextContents()
  expect(labels).toContain('~')
  if (scrolls) {
    await bar.evaluate((node) => node.scrollTo({ left: node.scrollWidth }))
    const last = await page.locator('.keybar > button').last().boundingBox()
    expect(last?.x ?? 0).toBeLessThan(page.viewportSize()?.width ?? 0)
  }
})

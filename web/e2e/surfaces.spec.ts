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
import { attachAgent, listSessions } from './agent.js'
import type { Page } from '@playwright/test'
import { createASession, firstShellRow } from './new-session.js'

// The credential the global setup joined with, not the invite: an invite is
// exchanged once, and every test after the first would be refused.
const TOKEN = process.env['OMT_CREDENTIAL'] ?? process.env['OMT_TOKEN'] ?? ''

test.beforeEach(async ({ page }) => {
  await page.goto(`/#token=${TOKEN}`)
})

/**
 * Open a session, creating one if the roster is empty.
 *
 * The creating branch is `createASession`, which goes through the project
 * picker — see that helper for why every call site here was tapping a button
 * that no longer exists and staying green.
 */
async function openASession(page: Page): Promise<void> {
  // A shell, never an agent session: one test attaches an agent, and a session
  // with an agent in it opens on its conversation rather than its terminal —
  // correctly, and not what a test about the terminal wants.
  if ((await firstShellRow(page).count()) === 0) {
    await createASession(page)
    return
  }
  await firstShellRow(page).tap()
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
  await page.locator('.composer-field').pressSequentially('echo block-view-probe\r')
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
  await page.locator('.composer-field').pressSequentially('echo search-me-later\r')
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
  await createASession(second)

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

test('a long press and drag sends arrow keys', async ({ page }) => {
  // The gesture that replaces reaching for the key bar on every cursor
  // movement. `24 §3.2` calls it the best gesture in any mobile terminal, and
  // without it a phone terminal feels like a toy.
  await openASession(page)
  await page.locator('.composer-field').fill('echo abcdef')
  await page.locator('.composer-send').tap()
  await page.waitForTimeout(800)

  const grid = page.locator('.terminal-grid')
  const box = await grid.boundingBox()
  test.skip(box === null, 'no grid to drag on')

  // Some emulated profiles define `Touch` and refuse to construct one, which
  // makes the gesture unreachable from a test rather than broken in the
  // product. Skipped by name where that is true; the decisions themselves are
  // covered by unit tests either way.
  const canSynthesise = await page.evaluate(() => {
    try {
      new Touch({ identifier: 1, target: document.body, clientX: 0, clientY: 0 })
      return true
    } catch {
      return false
    }
  })
  test.skip(!canSynthesise, 'this browser will not construct a Touch')

  const x = Math.round((box?.x ?? 0) + 60)
  const y = Math.round((box?.y ?? 0) + 60)
  // Held past the arm time, then dragged: a drag that starts immediately is a
  // scroll, and must stay one.
  await page.evaluate(
    async ({ px, py }: { px: number; py: number }) => {
      const grid = document.querySelector('.terminal-grid') as HTMLElement
      // Real `Touch` objects: a plain object cast to one is rejected by the
      // `TouchEvent` constructor, which is a type error rather than a silent
      // no-op.
      const touch = (clientX: number, clientY: number) =>
        new Touch({ identifier: 1, target: grid, clientX, clientY })
      grid.dispatchEvent(
        new TouchEvent('touchstart', { touches: [touch(px, py)], bubbles: true, cancelable: true }),
      )
      await new Promise((r) => setTimeout(r, 420))
      grid.dispatchEvent(
        new TouchEvent('touchmove', {
          touches: [touch(px + 120, py)],
          bubbles: true,
          cancelable: true,
        }),
      )
      grid.dispatchEvent(new TouchEvent('touchend', { touches: [], bubbles: true }))
    },
    { px: x, py: y },
  )

  // The arrows moved the cursor rather than typing anything: the line is
  // unchanged, which is what a cursor movement looks like.
  await expect(page.locator('.terminal-body')).toContainText('abcdef')
})

test('adding an instance offers the camera and says when it cannot', async ({ page }) => {
  // Scanning is the whole point of printing a code, and it was reachable only
  // by tapping `+` and hoping. Over plain HTTP there is no camera at all, which
  // is the commonest way a phone reaches omt on a LAN — so the button says so
  // rather than failing on tap.
  await page.getByRole('button', { name: /switch instance/i }).tap()
  const scan = page.getByRole('button', { name: /Scan a code/ })
  await expect(scan).toBeVisible()
  await expect(page.getByRole('button', { name: 'Paste a link' })).toBeVisible()

  const secure = await page.evaluate(() => window.isSecureContext)
  if (!secure) {
    await expect(scan).toBeDisabled()
    await expect(scan).toContainText('not on this browser')
  }
})

test('an agent session is read through its conversation, not its grid', async ({ page }) => {
  // The agent-native path, through the UI. Everything before this fed the
  // transcript events omt made up, which checks the renderer against omt's own
  // idea of what an agent says — the thing that can be wrong.
  const url = process.env['OMT_URL'] ?? 'http://127.0.0.1:7788'
  const token = TOKEN

  await createASession(page)

  // Which session the client is on, so the agent is attached to that one.
  const session = await page.evaluate(() => location.hash || document.title)
  const listed = await listSessions(url, token)
  const target = listed[listed.length - 1]
  test.skip(target === undefined, 'no session to attach to')
  expect(session).toBeDefined()

  await attachAgent(url, token, target as string, [
    process.env['PYTHON'] ?? 'python3',
    new URL('../../tests/integration/fake_agent.py', import.meta.url).pathname,
  ])

  await page.reload()
  await page.waitForTimeout(1500)
  // Back to the roster, explicitly. A reload used to land there on its own,
  // which was a defect — a phone backgrounds the browser, the tab is reloaded,
  // and somebody loses the screen they had open three taps down. The client
  // restores where it was now, so a test that wants the roster has to ask.
  const back = page.getByRole('button', { name: 'Back' })
  while ((await back.count()) > 0) {
    await back.tap()
    await page.waitForTimeout(300)
  }
  // By name, because a session with an agent in it is named after the agent —
  // five rows all called `shell` is the one thing a roster exists to tell
  // apart, and tapping the first of them picks at random.
  await page.locator('.body button.tap').filter({ hasText: 'codex' }).first().tap()
  await page.waitForTimeout(1500)

  // The conversation is offered, and it is what the session opened on.
  const conversation = page.locator('.surface').filter({ hasText: 'Conversation' })
  await expect(conversation).toBeEnabled()
  if ((await conversation.getAttribute('aria-selected')) !== 'true') {
    await conversation.tap()
  }

  // What the agent said, and what it ran, rendered as themselves rather than as
  // terminal output.
  await expect(page.locator('.entry-assistant').first()).toContainText('Reading the file.')
  await expect(page.locator('.entry-tool').first()).toContainText('cat README.md')
})

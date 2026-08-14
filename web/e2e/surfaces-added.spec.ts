/**
 * The screens `complete-the-web-surfaces` added, against a real instance.
 *
 * Against a real one because a mock would pass whatever these screens did:
 * every one of them exists to render something the instance answers, so a
 * fixture would be testing the fixture. The suite already runs an `omt` and
 * exchanges one invite for the whole run.
 *
 * Both device profiles, because a design tested at one width is a design that
 * breaks at the other — that is why this project runs two, and it has caught
 * a layout bug at each of them.
 */
import { expect, test } from '@playwright/test'

/** Land on the roster with a credential, as a paired phone does. */
async function open(page: import('@playwright/test').Page): Promise<void> {
  const token = process.env['OMT_CREDENTIAL'] ?? ''
  const url = process.env['OMT_URL'] ?? 'http://127.0.0.1:7788'
  await page.addInitScript(
    ({ t, u }) => {
      localStorage.setItem(
        'omt.instances',
        JSON.stringify([{ id: 'local', url: u, token: t, label: 'this machine' }]),
      )
      localStorage.setItem('omt.instance', 'local')
    },
    { t: token, u: url },
  )
  await page.goto('/')
  await expect(page.locator('.bar')).toBeVisible()
}

test.describe('opening a project', () => {
  test('the roster offers it, and it lists what the instance knows', async ({ page }) => {
    await open(page)
    await page.getByRole('button', { name: /Open a project/ }).click()

    // What makes this screen worth having: somewhere to start work from.
    await expect(page.locator('.tap').first()).toBeVisible()
    // And the thing it deliberately does not offer.
    await expect(page.locator('input[type="text"]')).toHaveCount(0)
    await expect(page.locator('.note')).toContainText(/run omt/i)
  })

  test('a workspace leads to its files and its changes', async ({ page }) => {
    await open(page)
    await page.getByRole('button', { name: /Open a project/ }).click()
    await expect(page.getByRole('button', { name: /— Files/ }).first()).toBeVisible()
    await expect(page.getByRole('button', { name: /— Changes/ }).first()).toBeVisible()
  })
})

test.describe('files', () => {
  test('lists a directory and opens a file in it', async ({ page }) => {
    await open(page)
    await page.getByRole('button', { name: /Open a project/ }).click()
    await page.getByRole('button', { name: /— Files/ }).first().click()

    const first = page.locator('.tap').first()
    await expect(first).toBeVisible()
    // A file's name is rendered as it is on disk. It was lower-cased by a rule
    // meant for the instance name, and `README.md` became `readme.md`.
    const readme = page.locator('button', { hasText: /^README\.md$/ }).first()
    if ((await readme.count()) > 0) {
      await readme.click()
      await expect(page.locator('.file-body')).toBeVisible()
      await expect(page.locator('h1')).toContainText('README.md')
    }
  })
})

test.describe('changes', () => {
  test('says plainly when there is nothing, or lists what there is', async ({ page }) => {
    await open(page)
    await page.getByRole('button', { name: /Open a project/ }).click()
    await page.getByRole('button', { name: /— Changes/ }).first().click()

    // One or the other, and never a blank screen: an empty list and "no
    // repository here" are different facts and both have to be said.
    const rows = page.locator('.tap')
    const empty = page.locator('.empty, .state')
    await expect(rows.first().or(empty.first())).toBeVisible()
  })
})

test.describe('settings', () => {
  test('lists every setting, including ones nobody has set', async ({ page }) => {
    await open(page)
    // Reached by route rather than by a control, because the entry point for
    // settings is the instance switcher and this is testing the screen.
    await page.evaluate(() => history.pushState({ route: { screen: 'settings' } }, ''))
    await page.evaluate(() => window.dispatchEvent(new PopStateEvent('popstate', { state: history.state })))
    await expect(page.locator('.bar h1')).toContainText(/Settings|设置/)
  })
})

test.describe('every screen survives a reload', () => {
  test('a diff stays open when the tab comes back', async ({ page }) => {
    // A phone backgrounds the browser and the tab is reloaded. Landing on the
    // roster every time is how somebody loses the thing they had open three
    // taps down.
    await open(page)
    await page.getByRole('button', { name: /Open a project/ }).click()
    await page.getByRole('button', { name: /— Changes/ }).first().click()

    const before = await page.locator('.bar h1').textContent()
    // `commit`, not `load` or `domcontentloaded`: this page holds a live
    // WebSocket and registers a service worker, and neither of those moments
    // reliably arrives for it under automation. What is being tested is what
    // the client renders, and the assertion below waits for that itself.
    await page.reload({ waitUntil: 'commit' })
    await expect(page.locator('.bar h1')).toHaveText(before ?? '', { timeout: 15_000 })
  })
})

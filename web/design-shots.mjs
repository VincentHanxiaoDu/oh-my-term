/**
 * Screenshot the real client, for looking at.
 *
 * Design review needs a picture. Reading a stylesheet tells you what the rules
 * say; only a render tells you what somebody sees, and every visual bug this
 * client has shipped was invisible in the source and obvious in a screenshot.
 *
 * Runs against a real instance, like the e2e suite does, because a mocked one
 * renders the empty state and the empty state is not the thing being reviewed.
 *
 *   omt serve --web 127.0.0.1:7788
 *   node design-shots.mjs <invite> [outdir]
 */
import { chromium, devices } from '@playwright/test'
import WebSocket from 'ws'

const url = process.env['OMT_URL'] ?? 'http://127.0.0.1:7788'
const invite = process.argv[2]
const outDir = process.argv[3] ?? '/tmp'

if (!invite) {
  console.error('usage: node design-shots.mjs <invite> [outdir]')
  process.exit(1)
}

/** Swap the invite for a credential, exactly as a phone does. */
const credential = await new Promise((resolve, reject) => {
  const socket = new WebSocket(`${url.replace(/^http/, 'ws')}/api/ws`)
  socket.on('open', () =>
    socket.send(
      JSON.stringify({
        t: 'join_exchange',
        request: { device: crypto.randomUUID(), n: 1 },
        invite,
        device: { name: 'design review', platform: 'tests' },
      }),
    ),
  )
  socket.on('message', (data) => {
    const message = JSON.parse(data.toString())
    socket.close()
    if (message.t === 'join_credential' && message.token) {
      resolve(message.token)
    } else {
      reject(new Error(data.toString()))
    }
  })
  socket.on('error', reject)
})

// Both themes and both shapes. A design reviewed at one width and one scheme is
// a design that breaks at the other, every time.
const shots = [
  ['phone-dark', { ...devices['iPhone 13'], colorScheme: 'dark' }],
  ['phone-light', { ...devices['iPhone 13'], colorScheme: 'light' }],
  ['desktop-dark', { viewport: { width: 1440, height: 900 }, colorScheme: 'dark' }],
  ['desktop-light', { viewport: { width: 1440, height: 900 }, colorScheme: 'light' }],
]

const browser = await chromium.launch()
for (const [name, device] of shots) {
  // The service worker serves the last build. Blocked, or a screenshot is of
  // whatever the client looked like the previous time somebody looked.
  const context = await browser.newContext({ ...device, serviceWorkers: 'block' })
  const page = await context.newPage()
  await page.addInitScript(
    ({ token, origin }) => {
      localStorage.setItem(
        'omt.instances',
        JSON.stringify([{ id: 'local', url: origin, token, label: 'this machine' }]),
      )
      localStorage.setItem('omt.instance', 'local')
    },
    { token: credential, origin: url },
  )
  await page.goto(url, { waitUntil: 'commit' })
  await page.waitForTimeout(2500)
  await page.screenshot({ path: `${outDir}/omt-${name}-roster.png` })

  // Every screen this client has a route to, so a design pass looks at all of
  // them rather than at the one somebody remembered. A screen that cannot be
  // reached is skipped rather than failing the run: this is a review tool, and
  // a picture of four screens is worth more than an error about the fifth.
  for (const [screen, open] of [
    ['projects', async () => {
      await page.getByRole('button', { name: /Open a project|打开项目/ }).click()
    }],
    ['files', async () => {
      await page.getByRole('button', { name: /— (Files|文件)/ }).first().click()
    }],
    ['changes', async () => {
      await page.goBack()
      await page.getByRole('button', { name: /— (Changes|改动)/ }).first().click()
    }],
  ]) {
    try {
      await open()
      await page.waitForTimeout(1600)
      await page.screenshot({ path: `${outDir}/omt-${name}-${screen}.png` })
      console.log(`${outDir}/omt-${name}-${screen}.png`)
    } catch {
      console.log(`${outDir}/omt-${name}-${screen}.png — not reachable, skipped`)
    }
  }
  console.log(`${outDir}/omt-${name}-roster.png`)
  await context.close()
}
await browser.close()

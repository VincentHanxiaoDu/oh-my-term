/**
 * What a phone can do about a session whose process is gone.
 *
 * Against a real instance, and never a fixture: a fixture that claimed a
 * session was orphaned would be testing the fixture, and the whole question
 * here is what the client does when the *instance* says a session's process is
 * gone.
 *
 * An orphan is born exactly one way — omt restores a snapshot and the pty it
 * describes is not there any more — so these cases need an instance that has
 * been restarted at least once:
 *
 *   S=$(mktemp -d)/state.json
 *   OMT_STATE_FILE=$S omt serve --web 127.0.0.1:7788   # create a session, stop it
 *   OMT_STATE_FILE=$S omt serve --web 127.0.0.1:7788   # that session is now an orphan
 *
 * On a first-run instance they skip rather than fail. A skip that says why is
 * honest; a green tick over an assertion that never ran is not, which is the
 * failure mode this whole change came out of.
 */
import { expect, test } from '@playwright/test'
import { createASession } from './new-session.js'
import { attachAgent } from './agent.js'

const TOKEN = process.env['OMT_CREDENTIAL'] ?? process.env['OMT_TOKEN'] ?? ''
const URL = process.env['OMT_URL'] ?? 'http://127.0.0.1:7788'

/**
 * Every session the instance is holding, with its state.
 *
 * The same handshake `agent.ts` uses — hello, wait for welcome, then call —
 * because that is the protocol. A socket that sends a call before the welcome
 * is answered with nothing, and a test that hangs says nothing about the
 * product.
 */
async function sessions(): Promise<{ id: string; state: string }[]> {
  const WebSocket = (await import('ws')).default
  const socket = new WebSocket(`${URL.replace(/^http/, 'ws')}/api/ws`, {
    headers: { Authorization: `Bearer ${TOKEN}` },
  })
  return await new Promise((resolve, reject) => {
    const giveUp = setTimeout(() => reject(new Error('the instance never answered')), 20_000)
    socket.on('open', () => {
      socket.send(JSON.stringify({ t: 'hello', proto: 1, client: 'e2e-orphans', token: TOKEN }))
    })
    socket.on('message', (data: Buffer) => {
      const message = JSON.parse(data.toString()) as {
        t: string
        output?: { sessions?: { id: string; state: string }[] }
      }
      if (message.t === 'welcome') {
        socket.send(
          JSON.stringify({
            t: 'call',
            request: { device: crypto.randomUUID(), n: 1 },
            capability: 'session.list',
            input: {},
          }),
        )
        return
      }
      if (message.t === 'result') {
        clearTimeout(giveUp)
        socket.close()
        resolve(message.output?.sessions ?? [])
      }
    })
    socket.on('error', reject)
  })
}

test.describe('a session whose process is gone', () => {
  test('says so, and offers a way back and a way out', async ({ page }) => {
    await page.goto(`/#token=${TOKEN}`)
    await createASession(page)

    const orphan = (await sessions()).find((s) => s.state === 'orphaned')
    test.skip(orphan === undefined, 'no orphaned session on this instance')

    // Reached by route, because getting there through the roster depends on
    // how many sessions this instance happens to be holding.
    await page.evaluate((id) => {
      history.pushState({ route: { screen: 'terminal', session: id } }, '')
      window.dispatchEvent(new PopStateEvent('popstate', { state: history.state }))
    }, orphan?.id ?? '')
    await page.waitForTimeout(1200)

    // The state, explained rather than named, and both ways out of it.
    await expect(page.getByRole('button', { name: /Run it again|重新运行/ })).toBeVisible()
    await expect(page.getByRole('button', { name: /Close this session|关闭此会话/ })).toBeVisible()
  })

  test('the roster offers to close all of them, and says how many', async ({ page }) => {
    await page.goto(`/#token=${TOKEN}`)
    await expect(page.locator('.bar')).toBeVisible()

    const dead = (await sessions()).filter((s) => s.state === 'orphaned').length
    const clear = page.getByRole('button', { name: /Close \d+ ended session|关闭 \d+ 个已结束的会话/ })

    if (dead === 0) {
      // Absent rather than present and inert: a control that does nothing
      // teaches people that controls do nothing.
      await expect(clear).toHaveCount(0)
      return
    }
    // The number is the confirmation. It is on the control itself rather than
    // in a dialog afterwards, because "are you sure" says nothing.
    await expect(clear).toBeVisible()
    await expect(clear).toContainText(String(dead))
  })

  test('clearing them never closes something that is running', async ({ page }) => {
    await page.goto(`/#token=${TOKEN}`)
    await createASession(page)
    // Back to the roster, where the control is.
    const back = page.getByRole('button', { name: 'Back' })
    while ((await back.count()) > 0) {
      await back.tap()
      await page.waitForTimeout(300)
    }

    const before = await sessions()
    const live = before.filter((s) => s.state !== 'orphaned').map((s) => s.id)
    const clear = page.getByRole('button', { name: /Close \d+ ended session|关闭 \d+ 个已结束的会话/ })
    test.skip((await clear.count()) === 0, 'nothing to clear on this instance')

    await clear.tap()
    await page.waitForTimeout(2500)

    // Every session that was not an orphan is still there. This is the one
    // code path in the client that closes several things at once, and
    // `session.close` on a running session kills a process.
    const after = new Set((await sessions()).map((s) => s.id))
    for (const id of live) {
      expect(after.has(id), `a live session was closed: ${id}`).toBe(true)
    }
  })
})

test.describe('attaching a file the instance already has', () => {
  /**
   * Land on an agent's conversation, which is where the attach sheet lives.
   *
   * 08 §6.3 puts it on the agent composer and not on the terminal's, and that
   * is right: a plain shell has nothing to hand a file to. The first version
   * of these tests opened a shell, found no attach button, and would have been
   * read as a missing feature.
   */
  async function anAgentConversation(page: import('@playwright/test').Page): Promise<boolean> {
    await page.goto(`/#token=${TOKEN}`)
    await createASession(page)
    const listed = await sessions()
    const target = listed[listed.length - 1]
    if (target === undefined) {
      return false
    }
    await attachAgent(URL, TOKEN, target.id, [
      process.env['PYTHON'] ?? 'python3',
      new global.URL('../../tests/integration/fake_agent.py', import.meta.url).pathname,
    ])
    await page.waitForTimeout(1500)
    await page.evaluate((id) => {
      history.pushState({ route: { screen: 'transcript', session: id } }, '')
      window.dispatchEvent(new PopStateEvent('popstate', { state: history.state }))
    }, target.id)
    await page.waitForTimeout(1200)
    return true
  }

  test('the sheet offers it, and choosing one returns to the session', async ({ page }) => {
    test.skip(!(await anAgentConversation(page)), 'no session to attach an agent to')

    await page.getByRole('button', { name: /Attach|附件/ }).tap()
    const row = page.getByRole('button', { name: /A file in this project|项目中的文件/ })
    await expect(row).toBeVisible()
    await row.tap()

    // The screen says what a tap will do, rather than looking like browsing.
    await expect(page.locator('.bar h1')).toContainText(/Choose a file|选择要附加的文件/)

    // A file, not a directory: a directory still opens, because that is how
    // you get to the file.
    const file = page.locator('.body button.tap').filter({ hasNotText: '/' }).first()
    test.skip((await file.count()) === 0, 'no plain file at the workspace root')
    await file.tap()

    // Off the picker and back to the session. Nothing was uploaded: the
    // instance already had the file.
    await page.waitForTimeout(1500)
    await expect(page.locator('.bar h1')).not.toContainText(/Choose a file|选择要附加的文件/)
  })

  test('the sheet has all five rows, and three separate file inputs', async ({ page }) => {
    // 09 §4.3.3: `capture` replaces the photo picker rather than being a mode
    // of it, so the library, the camera and the filesystem are three elements
    // and three rows. This shipped with two.
    test.skip(!(await anAgentConversation(page)), 'no session to attach an agent to')
    await page.getByRole('button', { name: /Attach|附件/ }).tap()

    for (const name of [
      /Photo library|照片图库/,
      /Take a photo|拍照/,
      /Choose a file|选择文件/,
      /A file in this project|项目中的文件/,
      /Paste an image|粘贴图片/,
    ]) {
      await expect(page.getByRole('button', { name })).toBeVisible()
    }

    // Three inputs, distinguishable by what each one opens.
    const inputs = await page.locator('input[type="file"]').evaluateAll((nodes) =>
      nodes.map((n) => ({
        accept: n.getAttribute('accept') ?? '',
        capture: n.getAttribute('capture') ?? '',
      })),
    )
    expect(inputs).toContainEqual({ accept: 'image/*', capture: '' })
    expect(inputs).toContainEqual({ accept: 'image/*', capture: 'environment' })
    expect(inputs).toContainEqual({ accept: '', capture: '' })
  })

  test('leaving the picker cancels the pick', async ({ page }) => {
    // Otherwise it stays armed, and the next file somebody opens to *read* is
    // sent to an agent.
    test.skip(!(await anAgentConversation(page)), 'no session to attach an agent to')

    await page.getByRole('button', { name: /Attach|附件/ }).tap()
    await page.getByRole('button', { name: /A file in this project|项目中的文件/ }).tap()
    await expect(page.locator('.bar h1')).toContainText(/Choose a file|选择要附加的文件/)
    await page.getByRole('button', { name: 'Back' }).tap()
    await page.waitForTimeout(600)
    await expect(page.locator('.bar h1')).not.toContainText(/Choose a file|选择要附加的文件/)
  })
})

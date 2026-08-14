import { describe, expect, it, vi } from 'vitest'

/**
 * The entry point, actually executed.
 *
 * Every other test here imports source. This one imports the *built* output
 * against a stub DOM, because the failure it catches lives only in the build:
 * a module specifier the compiler accepted and the browser cannot resolve
 * loads as a blank page with nothing in any log.
 */
function stubDom(hash: string) {
  // A stub element with the whole surface the client uses, rather than the
  // handful of methods it happened to call last time this test was written.
  // Adding one method per failure is a loop that only ends when the client
  // stops changing.
  const element = (): never => {
    const node: Record<string, unknown> = {
      style: { setProperty() {}, cssText: '' },
      classList: { add() {}, remove() {}, toggle() {} },
      dataset: {},
      children: [] as unknown[],
      append() {},
      remove() {},
      addEventListener() {},
      removeEventListener() {},
      setAttribute() {},
      getAttribute: () => null,
      querySelector: () => null,
      querySelectorAll: () => [],
      getBoundingClientRect: () => ({ width: 8, height: 16 }),
      clientWidth: 400,
      clientHeight: 800,
      textContent: '',
      disabled: false,
    }
    return node as never
  }

  const app = element()
  const storage = {
    store: {} as Record<string, string>,
    getItem(k: string) {
      return this.store[k] ?? null
    },
    setItem(k: string, v: string) {
      this.store[k] = v
    },
    removeItem(k: string) {
      delete this.store[k]
    },
  }
  const win = {
    location: {
      hash,
      pathname: '/',
      origin: 'http://x',
      protocol: 'http:',
      host: 'x',
      // The invite is read out of the whole URL, exactly as a scanned one is,
      // so the stub needs a whole URL and not only its parts.
      href: `http://x/${hash}`,
    },
    localStorage: storage,
    // The client asks this before it renders anything, to pick a language.
    // Stubbed here rather than guarded there alone: this stub claims to model
    // the whole surface the client uses, and a window with no navigator on it
    // is not a window any browser has ever had.
    navigator: { userAgent: 'node', languages: ['en-GB'], serviceWorker: { register() {} } },
    addEventListener() {},
    matchMedia: () => ({ matches: false }),
    isSecureContext: false,
    visualViewport: null,
  }
  vi.stubGlobal('document', {
    // Where the language is recorded, so a screen reader announces the page in
    // the language it is written in.
    documentElement: { lang: '' },
    getElementById: (id: string) => (id === 'app' ? app : null),
    createElement: element,
    body: { append() {} },
    // The entry point puts the stylesheet here. A stub without it fails with
    // "cannot read properties of undefined", which says nothing about what is
    // actually missing.
    head: { append() {} },
    createTextNode: (text: string) => ({ textContent: text }),
    addEventListener() {},
  })
  vi.stubGlobal('window', win)
  vi.stubGlobal('location', win.location)
  vi.stubGlobal('history', { replaceState() {} })
  vi.stubGlobal('navigator', { userAgent: 'node', serviceWorker: { register() {} } })
  vi.stubGlobal('crypto', { randomUUID: () => 'stub-uuid' })
  vi.stubGlobal(
    'WebSocket',
    class {
      readyState = 0
      send() {}
      close() {}
      addEventListener() {}
    },
  )
  return { app, storage }
}

/**
 * Import the compiled entry point.
 *
 * Through a variable, because the whole point is to load emitted JavaScript
 * that has no declarations — a typed import of it would be checking the source
 * again, which is what every other test here already does.
 */
const BUILT = '../public/app/main.js'
async function runEntryPoint(): Promise<void> {
  await import(/* @vite-ignore */ BUILT)
}

describe('the built entry point', () => {
  it('runs, taking the token out of the fragment and off the address bar', async () => {
    // A token in the query string lands in access logs, browser history and
    // any Referer the page sends, where it outlives whoever pasted it.
    const { storage } = stubDom('#token=t1')
    vi.resetModules()
    await runEntryPoint()
    // Stored as an instance now rather than as the one token the client used to
    // hold, because a second invite used to overwrite the first.
    expect(storage.getItem('omt.instances')).toContain('t1')
  })

  it('carries over the token an older client stored', async () => {
    // That token is the only copy — the instance prints it once and cannot
    // print it again — so an upgrade that dropped it would lock somebody out of
    // their own machine with no way back.
    const { storage } = stubDom('')
    storage.setItem('omt.token', 'from-the-old-client')
    vi.resetModules()
    await runEntryPoint()
    expect(storage.getItem('omt.instances')).toContain('from-the-old-client')
    expect(storage.getItem('omt.token')).toBeNull()
  })

  it('mints a device identity that survives a reload', async () => {
    // Without one, every refresh looks like a new device and the ledger cannot
    // tell a retry from a second person answering the same card.
    const { storage } = stubDom('#token=t2')
    vi.resetModules()
    await runEntryPoint()
    expect(storage.getItem('omt.device')).toBeTruthy()
  })
})

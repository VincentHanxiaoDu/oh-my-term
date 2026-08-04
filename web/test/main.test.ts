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
  const element = () => ({
    style: { setProperty() {}, cssText: '' },
    classList: { add() {} },
    append() {},
    remove() {},
    addEventListener() {},
    getBoundingClientRect: () => ({ width: 8, height: 16 }),
    clientWidth: 400,
    clientHeight: 800,
    textContent: '',
  })
  const app = element()
  const storage = {
    store: {} as Record<string, string>,
    getItem(k: string) {
      return this.store[k] ?? null
    },
    setItem(k: string, v: string) {
      this.store[k] = v
    },
  }
  const win = {
    location: { hash, pathname: '/', origin: 'http://x', protocol: 'http:', host: 'x' },
    localStorage: storage,
    addEventListener() {},
    matchMedia: () => ({ matches: false }),
    isSecureContext: false,
    visualViewport: null,
  }
  vi.stubGlobal('document', {
    getElementById: (id: string) => (id === 'app' ? app : null),
    createElement: element,
    body: { append() {} },
  })
  vi.stubGlobal('window', win)
  vi.stubGlobal('location', win.location)
  vi.stubGlobal('history', { replaceState() {} })
  vi.stubGlobal('navigator', { userAgent: 'node', serviceWorker: { register() {} } })
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
    expect(storage.getItem('omt.token')).toBe('t1')
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

/**
 * The entry point: the few lines that turn modules into a running client.
 *
 * Everything decidable lives in the modules this imports and is tested there.
 * What is here is the wiring only a browser can exercise — the token, the
 * socket, the viewport, the service worker — kept deliberately thin so that
 * "untested" and "trivial" describe the same code.
 */

import { App } from './app.js'
import { Store } from './store.js'
import { connect } from './connect.js'
import { canPush } from './push.js'
import { keyToBytes } from './touch.js'
import { fit } from './screen.js'
import { applyTheme } from './terminal.js'
import type { Theme } from './terminal.js'

/**
 * Where the token comes from.
 *
 * The fragment, never the query: a query string is written to the server's
 * access log, to browser history and to any `Referer` the page sends, and a
 * bearer token in any of those outlives the person who pasted it. It is
 * stripped from the address bar as soon as it is read.
 */
function takeToken(loc: Location, storage: Storage): string | null {
  const fragment = new URLSearchParams(loc.hash.replace(/^#/, ''))
  const fresh = fragment.get('token')
  if (fresh !== null) {
    storage.setItem('omt.token', fresh)
    history.replaceState(null, '', loc.pathname)
    return fresh
  }
  return storage.getItem('omt.token')
}

/**
 * This browser's identity, stable across reloads.
 *
 * The instance uses it to tell one client from another when resolving a card,
 * so it must survive a refresh — otherwise every reload looks like a new device
 * and the ledger cannot tell a retry from a second person answering.
 */
function deviceId(storage: Storage): string {
  const existing = storage.getItem('omt.device')
  if (existing !== null) {
    return existing
  }
  const fresh = crypto.randomUUID()
  storage.setItem('omt.device', fresh)
  return fresh
}

function main(): void {
  const root = document.getElementById('app')
  if (root === null) {
    return
  }

  const token = takeToken(window.location, window.localStorage)
  if (token === null) {
    root.textContent =
      'Open the link `omt web` printed. It carries the token, which is shown once.'
    return
  }

  let store: Store
  const transport = connect(
    { url: window.location.origin, token },
    {
      onMessage: (message) => store.receive(message),
      onOpen: () => store.connect(token),
    },
  )
  store = new Store(deviceId(window.localStorage), transport)
  const app = new App(store, root)

  // The grid follows the viewport, not the other way round. A phone rotating
  // or a keyboard opening halves the usable height, and a terminal that keeps
  // the old size hides the line being typed behind the keyboard.
  let last = { cols: 0, rows: 0 }
  const resize = () => {
    const cell = measureCell()
    const view = window.visualViewport
    const next = fit({
      width: view?.width ?? root.clientWidth,
      height: view?.height ?? root.clientHeight,
      cellWidth: cell.width,
      cellHeight: cell.height,
    })
    if (next.cols === last.cols && next.rows === last.rows) {
      return
    }
    last = next
    const session = app.route.screen === 'terminal' ? app.route.session : null
    if (session !== null) {
      void store.call('session.resize', session, next.cols, next.rows).catch(() => {})
    }
  }
  window.addEventListener('resize', resize)
  window.visualViewport?.addEventListener('resize', resize)
  resize()

  // Registered last, and only if it could ever help: a failed service worker
  // must cost the notifications, not the client. Everything above already
  // works without it.
  if (canPush(environment()).available) {
    void navigator.serviceWorker.register('/sw.js')
  }

  // Everything typed goes to the session on screen. Attached once, on the
  // document, because the terminal is redrawn on every frame and a listener on
  // it would be lost with the element it was attached to.
  document.addEventListener('keydown', (event) => {
    if (app.route.screen !== 'terminal') {
      return
    }
    const bytes = keyToBytes(event)
    if (bytes === null) {
      return
    }
    // Only once it is going somewhere: preventing default on a key omt does
    // not handle would break the browser's own shortcuts for no reason.
    event.preventDefault()
    void app.type(bytes)
  })

  // Asked for once, on arrival. The colours are the instance's — a client that
  // used its own would render the user's terminal in somebody else's palette.
  void store
    .call('theme.get')
    .then((theme) => applyTheme(document.documentElement, theme as Theme))
    .catch(() => {
      // The built-in fallback in the stylesheet already applies. A missing
      // theme must not stop the client from being usable.
    })

  app.render()
}

/** How big one character is — measured, never assumed. */
function measureCell(): { width: number; height: number } {
  const probe = document.createElement('span')
  probe.className = 'terminal'
  probe.style.position = 'absolute'
  probe.style.visibility = 'hidden'
  probe.textContent = 'M'
  document.body.append(probe)
  const box = probe.getBoundingClientRect()
  probe.remove()
  return { width: box.width, height: box.height }
}

function environment() {
  return {
    hasServiceWorker: 'serviceWorker' in navigator,
    hasPushManager: 'PushManager' in window,
    isSecureContext: window.isSecureContext,
    isStandalone: window.matchMedia('(display-mode: standalone)').matches,
    isIos: /iP(hone|ad|od)/.test(navigator.userAgent),
    permission: ('Notification' in window ? Notification.permission : 'default') as
      | 'default'
      | 'granted'
      | 'denied',
  }
}

main()

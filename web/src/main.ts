/**
 * The entry point: the few lines that turn modules into a running client.
 *
 * Everything decidable lives in the modules this imports and is tested there.
 * What is here is the wiring only a browser can exercise — the token, the
 * socket, the viewport, the service worker — kept deliberately thin so that
 * "untested" and "trivial" describe the same code.
 */

import { App } from './app.js'
import { Navigator } from './nav.js'
import { STYLES } from './tokens.js'
import { Store } from './store.js'
import { connect } from './connect.js'
import { canPush } from './push.js'
import { choose, current, forget, list, readInvite, remember } from './instances.js'
import { describeDevice, exchangeInvite, isInvite } from './join.js'
import type { Instance } from './instances.js'
import { fit } from './screen.js'
import { applyTheme } from './terminal.js'
import type { Theme } from './terminal.js'

/**
 * Which instance to connect to, taking an arriving invite if there is one.
 *
 * The token comes from the fragment, never the query: a query string is written
 * to the server's access log, to browser history and to any `Referer` the page
 * sends, and a bearer token in any of those outlives the person who pasted it.
 * It is stripped from the address bar as soon as it is read.
 */
function takeInvite(loc: Location, storage: Storage): Instance | null {
  const arriving = readInvite(loc.href, () => crypto.randomUUID())
  if (arriving !== null) {
    // Stripped from the address bar immediately: a token left in the URL ends
    // up in a screenshot, in a shared tab, and in whatever the browser syncs.
    history.replaceState(null, '', loc.pathname)
    remember(storage, arriving)
    choose(storage, arriving.id)
    return arriving
  }

  // A token stored by a client that only knew how to hold one. Migrated rather
  // than dropped: it is the only copy, and the instance will not print it again.
  const legacy = storage.getItem('omt.token')
  if (legacy !== null && list(storage).length === 0) {
    const migrated: Instance = {
      id: crypto.randomUUID(),
      label: loc.host,
      url: loc.origin,
      token: legacy,
    }
    remember(storage, migrated)
    choose(storage, migrated.id)
    storage.removeItem('omt.token')
    return migrated
  }
  return current(storage)
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

async function main(): Promise<void> {
  const root = document.getElementById('app')
  if (root === null) {
    return
  }

  let instance = takeInvite(window.location, window.localStorage)

  // An invite is the right to obtain a credential, not a credential. Swapped
  // here, before anything connects, so what is stored is this device's own —
  // expiring, single-exchange, and individually revocable when a phone is lost.
  if (instance !== null && isInvite(instance.token)) {
    root.textContent = 'Joining…'
    const joined = await exchangeInvite(instance.url, instance.token, describeDevice())
    if (joined === null) {
      root.textContent =
        'That invite could not be used. It expires after a few minutes and works once — ask for a fresh one.'
      // Forgotten, or the next load retries a token that can never work and
      // reports the same failure forever.
      forget(window.localStorage, instance.id)
      return
    }
    instance = { ...instance, token: joined.token, label: joined.instance.name }
    remember(window.localStorage, instance)
    choose(window.localStorage, instance.id)
    root.textContent = ''
  }

  if (instance === null) {
    root.textContent =
      'Open the link `omt web` printed, or scan the code beside it. It carries the token, which is shown once.'
    return
  }
  const token = instance.token

  let store: Store
  const transport = connect(
    // The chosen instance's origin, which is not always this page's: a client
    // served by the laptop can be pointed at the desktop.
    { url: instance.url, token },
    {
      onMessage: (message) => store.receive(message),
      onOpen: () => store.connect(token),
    },
  )
  store = new Store(deviceId(window.localStorage), transport)
  // The stylesheet ships with the code rather than living in the HTML, so a
  // class name has one home and the two cannot drift.
  const style = document.createElement('style')
  style.textContent = STYLES
  document.head.append(style)

  const nav = new Navigator()
  const app = new App(store, root, nav)

  // Every *other* instance this browser knows about, connected read-only so the
  // roster is one list across all of them. Somebody with a laptop and a desktop
  // asks "does anything need me", not "does anything on the laptop need me" —
  // and a per-instance list makes them ask it twice and remember which one they
  // already checked.
  for (const other of list(window.localStorage)) {
    if (other.id === instance.id) {
      continue
    }
    let peer: Store
    const link = connect(
      { url: other.url, token: other.token },
      {
        onMessage: (message) => peer.receive(message),
        onOpen: () => peer.connect(other.token),
      },
    )
    peer = new Store(deviceId(window.localStorage), link)
    // Its rows join the roster; everything else still happens against the
    // instance in front of you. Opening a session on another machine switches
    // to it first, because a terminal is a connection and not a list entry.
    app.federate(other.id, other.label, peer)
  }

  // The instance's own theme, asked for once. `applyTheme` existed and was
  // imported here and never called, so every client rendered the built-in
  // palette and a configured theme did nothing — which looks correct, because
  // the default is a real theme.
  void store
    .call('theme.get')
    .then((theme) => {
      applyTheme(document.documentElement, theme as Theme)
    })
    // A theme that cannot be fetched leaves the defaults in place. There is
    // nothing useful to say about it and nothing broken.
    .catch(() => {})

  // The phone's back gesture is the one navigation control every user already
  // has. A web app that ignores it throws them out to the previous website
  // rather than the previous screen.
  window.addEventListener('popstate', (event) => nav.restore(event.state))

  // The grid follows the viewport, not the other way round. A phone rotating
  // or a keyboard opening halves the usable height, and a terminal that keeps
  // the old size hides the line being typed behind the keyboard.
  //
  // Keyed by session, not global. A pty is born 80 columns wide, so "the
  // viewport has not changed since last time" is not a reason to skip the
  // resize — it is a different pty, and on a 390px screen 80 columns means
  // half of every line is off the right-hand edge.
  let last: { session: string; cols: number; rows: number } | null = null
  const resize = () => {
    const session = app.route.screen === 'terminal' ? app.route.session : null
    if (session === null) {
      return
    }
    const cell = measureCell()
    const view = window.visualViewport
    // The box the grid is actually drawn in, when there is one. The viewport is
    // wider than it — by the body's padding — and asking for that many columns
    // puts the last few characters of every long line past the right edge.
    const box = document.querySelector('.terminal-grid')?.getBoundingClientRect()
    // Height from the scrolling body rather than the viewport: the viewport
    // also contains the app bar and the key bar, and counting those as terminal
    // rows puts the bottom of the screen underneath them.
    const area = document.querySelector('.terminal-body')?.clientHeight
    const next = fit({
      width: box?.width ?? view?.width ?? root.clientWidth,
      height: area ?? view?.height ?? root.clientHeight,
      cellWidth: cell.width,
      cellHeight: cell.height,
    })
    if (last?.session === session && last.cols === next.cols && last.rows === next.rows) {
      return
    }
    last = { session, ...next }
    void store.call('session.resize', session, next.cols, next.rows).catch(() => {})
  }
  window.addEventListener('resize', resize)
  window.visualViewport?.addEventListener('resize', resize)
  // Opening a session is the moment that matters, and it fires no resize event.
  // Without this the grid is only ever corrected by rotating the phone.
  nav.subscribe(() => resize())
  resize()

  // Registered last, and only if it could ever help: a failed service worker
  // must cost the notifications, not the client. Everything above already
  // works without it.
  if (canPush(environment()).available) {
    void navigator.serviceWorker.register('/sw.js')
  }

  // Typing is handled by the terminal screen's own input, which is what opens
  // the keyboard on a phone. A second listener here would double-send every
  // character on a desktop, where both fire.

  // The visual viewport shrinks when the keyboard opens. Without this the app
  // keeps its full height and the line being typed sits behind the keyboard.
  const fitToKeyboard = () => {
    const view = window.visualViewport
    if (view) {
      root.style.height = `${view.height}px`
    }
  }
  window.visualViewport?.addEventListener('resize', fitToKeyboard)
  window.visualViewport?.addEventListener('scroll', fitToKeyboard)
  fitToKeyboard()

  app.render()
}

/**
 * How big one character is — measured, never assumed.
 *
 * The probe takes the terminal's *font* and nothing else. Wearing the whole
 * `.terminal` class, it inherited that rule's `display: block` and padding, so
 * it measured the container: 24px wide and the full height of the screen. The
 * grid came out at the clamp minimums, 20 columns by 4 rows, and a phone showed
 * four lines of a session with everything else off the edge.
 *
 * Ten characters, divided: sub-pixel advance widths are real, and rounding one
 * character's width up by a fraction loses a column every eight characters.
 */
const SAMPLE = 'MMMMMMMMMM'
function measureCell(): { width: number; height: number } {
  const probe = document.createElement('span')
  probe.style.position = 'absolute'
  probe.style.visibility = 'hidden'
  probe.style.display = 'inline-block'
  probe.style.padding = '0'
  probe.style.border = '0'
  probe.style.whiteSpace = 'pre'
  // The terminal's font, taken from the terminal itself when one is on screen
  // so this cannot drift from what is actually being rendered.
  const drawn = document.querySelector('.terminal')
  probe.style.font = drawn ? getComputedStyle(drawn).font : 'var(--text-sm) ui-monospace, monospace'
  probe.textContent = SAMPLE
  document.body.append(probe)
  const box = probe.getBoundingClientRect()
  probe.remove()
  return { width: box.width / SAMPLE.length, height: box.height }
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

void main()

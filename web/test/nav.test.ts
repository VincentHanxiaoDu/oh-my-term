import { describe, expect, it } from 'vitest'
import { Navigator, hasBack, titleOf, type Route } from '../src/nav.js'

/** A history that records what it was given, as the browser's does. */
function fakeHistory() {
  const entries: unknown[] = []
  return {
    entries,
    pushState(state: unknown) {
      entries.push(state)
    },
    replaceState(state: unknown) {
      entries[entries.length - 1] = state
    },
    back() {},
  } as unknown as History & { entries: unknown[] }
}

/** Every route, with the arguments each needs. */
const everyRoute: Route[] = [
  { screen: 'roster' },
  { screen: 'terminal', session: 's1' },
  { screen: 'threads', session: 's1' },
  { screen: 'card', interaction: 'i1' },
  { screen: 'instances' },
  { screen: 'blocks', session: 's1' },
  { screen: 'transcript', session: 's1' },
  { screen: 'search', session: 's1' },
  { screen: 'projects' },
  { screen: 'files', workspace: 'w1', path: 'src/deep' },
  { screen: 'changes', workspace: 'w1' },
  { screen: 'diff', workspace: 'w1', path: 'src/lib.rs' },
  { screen: 'settings' },
]

describe('where the client is', () => {
  it('names every screen', () => {
    // A screen reachable and unnameable is one whose bar is blank, and the
    // exhaustive switch is only half the guarantee — it stops the code
    // compiling, not a title being an empty string.
    for (const route of everyRoute) {
      const title = titleOf(route, { title: 'context' })
      expect(title, `${route.screen} has no title`).toBeTruthy()
      expect(title.trim(), `${route.screen} has a blank title`).not.toBe('')
    }
  })

  it('gives every screen but the roster somewhere to go back to', () => {
    for (const route of everyRoute) {
      expect(hasBack(route), route.screen).toBe(route.screen !== 'roster')
    }
  })

  it('survives a reload on every screen', () => {
    // A phone backgrounds the browser and comes back, and the tab is reloaded.
    // Landing on the roster every time is how somebody loses the diff they had
    // open, three taps down.
    for (const route of everyRoute) {
      const history = fakeHistory()
      const nav = new Navigator(history)
      nav.push(route)
      const stored = history.entries[history.entries.length - 1]

      // What the browser hands back on `popstate` after a reload.
      const restored = new Navigator(fakeHistory())
      restored.restore(JSON.parse(JSON.stringify(stored)))
      expect(restored.route).toEqual(route)
    }
  })

  it('lands on the roster when the browser has nothing to restore', () => {
    // The entry the page loaded on, and anything a previous version of this
    // client wrote. Neither is a reason to render nothing.
    const nav = new Navigator(fakeHistory())
    for (const state of [null, undefined, {}, { route: undefined }, 'nonsense']) {
      nav.restore(state)
      expect(nav.route.screen).toBe('roster')
    }
  })

  it('titles a directory by its own name rather than the whole path', () => {
    // The workspace is named on the screen above. Repeating the path here
    // spends the one thing a title has on the answer to a question nobody
    // asked twice.
    expect(titleOf({ screen: 'files', workspace: 'w', path: 'crates/omt/src' })).toBe('src')
    expect(titleOf({ screen: 'diff', workspace: 'w', path: 'a/b/lib.rs' })).toBe('lib.rs')
  })

  it('titles the workspace root by what it is rather than by nothing', () => {
    expect(titleOf({ screen: 'files', workspace: 'w', path: '' }, { title: 'omt' })).toBe('omt')
  })
})

describe('a chooser is not somewhere to go back to', () => {
  it('leaves no step behind when it has been answered', () => {
    // The project picker exists to choose a workspace. Pushed, back from the
    // new session landed on the picker again and the way out was to press
    // back a second time — which on a phone reads as the button not having
    // worked the first time.
    const history = fakeHistory()
    const nav = new Navigator(history)
    nav.push({ screen: 'projects' })
    nav.replace({ screen: 'terminal', session: 's1' })

    // One entry for both, not two: the picker is gone rather than underneath.
    expect(history.entries).toHaveLength(1)
    expect(history.entries[0]).toEqual({ route: { screen: 'terminal', session: 's1' } })
  })

  it('still has somewhere to go back to', () => {
    // Replacing must not leave a screen with no way out. The roster is the
    // entry the client loaded on, and it is still there underneath.
    const nav = new Navigator(fakeHistory())
    nav.replace({ screen: 'terminal', session: 's1' })
    expect(hasBack(nav.route)).toBe(true)
  })
})

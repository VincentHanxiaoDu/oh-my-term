/**
 * The screens.
 *
 * Deliberately no framework. This is a few hundred lines of DOM against a store
 * that already holds every rule, and a framework here would be a dependency
 * whose upgrade cadence outlives the code it renders.
 *
 * The order of the screens is the argument of the whole client: the roster
 * first, because a phone is used for ninety seconds because something buzzed,
 * and a terminal that opens to a wall of text has already failed that.
 */

import { KEY_BAR, KEY_BAR_SECONDARY, applyLatch, confirmation, riskOf } from './touch.js'
import type { Interaction } from './protocol.js'
import type { Store, ViewState } from './store.js'
import { changed, paint } from './terminal.js'
import type { Snapshot } from './terminal.js'
import { trimTrailingBlank } from './screen.js'

/** Which screen is showing. */
export type Route =
  | { screen: 'roster' }
  | { screen: 'session'; session: string }
  | { screen: 'card'; interaction: string }
  | { screen: 'terminal'; session: string }

/** What a cell looks like in the DOM. */
export interface Rendered {
  /** The element's text. */
  text: string
  /** Its class, which carries the tone. */
  className: string
  /** What a screen reader says — never only colour. */
  label: string
}

/** How a session row reads. */
export function renderSessionRow(session: {
  title: string
  state: string
}): Rendered {
  const blocked = session.state === 'blocked'
  return {
    text: session.title,
    className: blocked ? 'row blocked' : 'row',
    label: `${session.title} — ${blocked ? 'needs you' : session.state}`,
  }
}

/**
 * The roster, ordered.
 *
 * Blocked first, and never by name or recency: spawn order buries the one row
 * that matters behind four that do not, which on a phone means scrolling to
 * find the only thing you opened the app for.
 */
export function orderRoster<T extends { state: string; title: string }>(rows: readonly T[]): T[] {
  const rank = (s: string) =>
    s === 'blocked' ? 0 : s === 'working' ? 1 : s === 'idle' ? 2 : 3
  return [...rows].sort(
    (a, b) => rank(a.state) - rank(b.state) || a.title.localeCompare(b.title),
  )
}

/** The header above the roster — one number. */
export function rosterHeader(state: ViewState): string {
  if (state.connection !== 'connected') {
    return state.refusal ?? `${state.connection}…`
  }
  if (state.needsYou > 0) {
    return `${state.needsYou} of ${state.sessions.length} need you`
  }
  return state.sessions.length === 0
    ? 'no sessions'
    : `${state.sessions.length} running`
}

/** What a card offers, and how hard each option is to choose. */
export interface CardOption {
  label: string
  /** How the option is confirmed. */
  confirm: 'tap' | 'hold'
  /** Whether choosing it is what the agent asked for by default. */
  isDefault: boolean
}

/**
 * Lay out a card's options.
 *
 * The agent's own options, verbatim and in its order — omt neither adds,
 * removes nor reorders them, because the indices are how the answer is
 * delivered. What omt adds is only *how hard each one is to choose*.
 */
export function layOutCard(
  options: readonly string[],
  command: string | undefined,
): CardOption[] {
  const risk = command === undefined ? 'ordinary' : riskOf(command)
  return options.map((label, index) => ({
    label,
    // Only the affirmative options inherit the command's risk. Making "Deny"
    // hard to press would push people toward the dangerous answer, which is
    // precisely backwards.
    confirm: isAffirmative(label) ? confirmation(risk) : 'tap',
    isDefault: index === 0,
  }))
}

function isAffirmative(label: string): boolean {
  return /^(yes|allow|approve|ok|confirm|proceed|continue)/i.test(label.trim())
}

/** Whether a card can be acted on here, and why not if not. */
export function cardStatus(interaction: Interaction): { answerable: boolean; why?: string } {
  if (interaction.deliverable.kind === 'none') {
    return { answerable: false, why: interaction.deliverable.reason }
  }
  if (interaction.state.state !== 'open') {
    return { answerable: false, why: 'it has already been answered' }
  }
  return { answerable: true }
}

/**
 * How often the terminal screen asks for a new picture.
 *
 * Chosen against the thing people actually notice: the gap between pressing a
 * key and seeing it. Faster than this buys nothing a person can perceive, and
 * costs a request every time.
 */
export const POLL_MS = 150

/** A tiny renderer, so the shell has no framework and no build step. */
export class App {
  #store: Store
  #root: HTMLElement
  #route: Route = { screen: 'roster' }
  #latched: 'ctrl' | 'alt' | null = null
  #screen: Snapshot | null = null
  #epoch = 0
  #poll: ReturnType<typeof setInterval> | null = null

  constructor(store: Store, root: HTMLElement) {
    this.#store = store
    this.#root = root
    store.subscribe(() => this.render())
  }

  /** Go somewhere. */
  go(route: Route): void {
    this.#route = route
    // The poll belongs to the terminal screen and to nothing else. Left
    // running, it keeps asking a phone's radio for a screen nobody is looking
    // at, which is the kind of thing that shows up as battery drain.
    if (this.#poll !== null) {
      clearInterval(this.#poll)
      this.#poll = null
    }
    if (route.screen === 'terminal') {
      const session = route.session
      this.#poll = setInterval(() => void this.refreshTerminal(session), POLL_MS)
    }
    this.render()
  }

  /**
   * Type into the session on screen.
   *
   * The epoch goes with every write. Input already in flight when the token
   * changed hands is rejected rather than landing in whatever the new holder
   * is composing — which is the entire reason the token exists.
   */
  async type(text: string): Promise<void> {
    if (this.#route.screen !== 'terminal') {
      return
    }
    try {
      await this.#store.call('session.write', this.#route.session, text, this.#epoch)
      await this.refreshTerminal(this.#route.session)
    } catch {
      // On the store's problem list already.
    }
  }

  /** Which screen is showing. */
  get route(): Route {
    return this.#route
  }

  /** A key from the accessory bar. */
  pressBarKey(index: number, secondary = false): string | null {
    const bar = secondary ? KEY_BAR_SECONDARY : KEY_BAR
    const key = bar[index]
    if (!key) {
      return null
    }
    if ('latch' in key.action) {
      // Tapping a latched modifier again releases it — otherwise a mis-tap
      // arms Ctrl and the next letter does something unexpected.
      this.#latched = this.#latched === key.action.latch ? null : key.action.latch
      return null
    }
    const bytes = applyLatch(this.#latched, key.action.send)
    this.#latched = null
    return bytes
  }

  /** Which modifier is armed, for the bar to show. */
  get latched(): 'ctrl' | 'alt' | null {
    return this.#latched
  }

  /** Draw. */
  render(): void {
    const state = this.#store.state
    this.#root.textContent = ''
    this.#root.append(this.#header(state))

    switch (this.#route.screen) {
      case 'roster':
        this.#root.append(this.#roster(state))
        break
      case 'session':
        this.#root.append(this.#session(this.#route.session))
        break
      case 'terminal':
        this.#root.append(this.#terminal(this.#route.session))
        break
      case 'card':
        this.#root.append(this.#card(state, this.#route.interaction))
        break
    }

    for (const problem of state.problems) {
      const note = document.createElement('p')
      note.className = problem.transient ? 'problem transient' : 'problem'
      note.textContent = problem.message
      this.#root.append(note)
    }
  }

  #header(state: ViewState): HTMLElement {
    const h = document.createElement('h1')
    h.textContent = rosterHeader(state)
    return h
  }

  #roster(state: ViewState): HTMLElement {
    const box = document.createElement('div')

    // Offered only when there is somewhere to put it. A button that opens a
    // form asking for a path the phone cannot browse is worse than no button.
    const workspace = state.workspaces[0]
    if (workspace !== undefined) {
      const create = document.createElement('button')
      create.className = 'create'
      create.textContent = 'New session'
      create.addEventListener('click', () => {
        void this.newSession(workspace.id)
      })
      box.append(create)
    }

    const list = document.createElement('ul')
    for (const session of orderRoster(
      state.sessions.map((s) => ({ ...s, title: s.title })),
    )) {
      const row = renderSessionRow(session)
      const item = document.createElement('li')
      const button = document.createElement('button')
      button.className = row.className
      button.textContent = row.text
      button.setAttribute('aria-label', row.label)
      button.addEventListener('click', () => {
        void this.#store.loadThreads(session.id)
        this.go({ screen: 'session', session: session.id })
      })
      item.append(button)
      list.append(item)
    }
    box.append(list)
    return box
  }

  /**
   * Start a session and go straight to it.
   *
   * Straight to it, because the reason somebody pressed this is that they want
   * to type — leaving them on the roster to find the row themselves adds a tap
   * to the one flow that should have none.
   */
  async newSession(workspace: string): Promise<void> {
    try {
      const out = (await this.#store.call('session.create', workspace)) as { session: string }
      // The writer token, immediately: without it every keystroke is refused,
      // and the refusal arrives only once the user has already typed something.
      const claim = (await this.#store.call('session.acquire', out.session)) as { epoch: number }
      this.#epoch = claim.epoch
      this.#screen = null
      await this.#store.refresh()
      this.go({ screen: 'terminal', session: out.session })
    } catch {
      // The store already put it on the problem list.
    }
  }

  #session(session: string): HTMLElement {
    const box = document.createElement('div')
    const summary = document.createElement('p')
    summary.textContent = this.#store.summaryFor(session)
    box.append(summary)

    const gridEl = document.createElement('div')
    gridEl.className = 'grid'
    for (const cell of this.#store.gridFor(session)) {
      const el = document.createElement('button')
      el.className = `cell ${cell.tone}`
      el.textContent = cell.glyph
      el.setAttribute('aria-label', cell.label)
      el.disabled = !cell.actionable && cell.tone !== 'working'
      const first = cell.thread.open_interactions[0]
      if (first !== undefined) {
        el.addEventListener('click', () => this.go({ screen: 'card', interaction: first }))
      }
      gridEl.append(el)
    }
    box.append(gridEl)
    return box
  }

  #terminal(session: string): HTMLElement {
    const host = document.createElement('div')
    host.className = 'terminal'
    if (this.#screen === null) {
      host.textContent = 'reading the screen…'
      void this.refreshTerminal(session)
    } else {
      paint(host, this.#screen)
    }
    return host
  }

  /**
   * Fetch the screen and redraw only if it moved.
   *
   * Pulled rather than pushed: a phone that has been in a pocket for an hour
   * wants the screen as it is now, not an hour of deltas to replay before it
   * can find out.
   */
  async refreshTerminal(session: string): Promise<void> {
    try {
      const next = (await this.#store.call('session.snapshot', session)) as Snapshot
      if (changed(this.#screen, next)) {
        this.#screen = next
        this.render()
      }
    } catch {
      // Already on the store's problem list, which is on screen. Reporting the
      // same failure twice is not more informative.
    }
  }

  #card(state: ViewState, id: string): HTMLElement {
    const box = document.createElement('div')
    const interaction = state.open.find((i) => i.id === id)
    if (!interaction) {
      box.textContent = 'that card is no longer open'
      return box
    }
    const status = cardStatus(interaction)
    if (!status.answerable) {
      // Shown read-only with the reason and a route to the terminal, rather
      // than a button that silently does nothing.
      const why = document.createElement('p')
      why.className = 'readonly'
      why.textContent = `omt cannot answer this here: ${status.why ?? 'unknown'}`
      box.append(why)
    }
    return box
  }
}

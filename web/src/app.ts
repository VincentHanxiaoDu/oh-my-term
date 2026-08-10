/**
 * The screens.
 *
 * Composed from `view.ts` rather than hand-built, so two screens cannot
 * disagree about what a row is. No framework: the store and the terminal
 * renderer are the hard parts and are tested without a DOM, and a diffing
 * algorithm for a list of six things is not worth a dependency whose upgrade
 * cadence outlives the code it renders.
 *
 * The order of the screens is the argument of the whole client: the roster
 * first, because a phone is picked up for ninety seconds because something
 * buzzed, and a terminal that opens to a wall of text has already failed that.
 */

import { KEY_BAR, KEY_BAR_SECONDARY, applyLatch, confirmation, riskOf } from './touch.js'
import type { InteractionCard } from './protocol.js'
import type { Store } from './store.js'
import { changed, paint } from './terminal.js'
import { keyToBytes } from './touch.js'
import type { Snapshot } from './terminal.js'
import { Navigator, hasBack, titleOf } from './nav.js'
import type { Route } from './nav.js'
import { appBar, el, renderState, row, stateOf } from './view.js'
import { choose, current, forget, list, readInvite, remember } from './instances.js'
import { canScan, scanCode } from './scan.js'
import { markElement, markFor } from './mark.js'
import { cardTitle, holdToConfirm, planElement, promptElement } from './cards.js'
import { dictate, dictationAvailable } from './dictate.js'
import {
  ARROWS,
  DOUBLE_TAP_MS,
  DRAG_ARM_MS,
  directionOf,
  fontForPinch,
  isDoubleTap,
  repeatsFor,
  spanOf,
} from './gestures.js'
import { unify, unifiedSubtitle } from './federation.js'
import type { InstanceRoster, UnifiedSession } from './federation.js'
import { pairCalls, renderEntry, surfacesFor } from './surfaces.js'
import type { TranscriptEntry } from './surfaces.js'
import type { ScreenState } from './view.js'

/** How often the terminal screen asks for a new picture. */
export const POLL_MS = 150

/**
 * How often the structured surfaces refresh.
 *
 * Far slower than the grid, because a block list and a transcript change when
 * something *happens* rather than on every keystroke echo, and a phone polling
 * either of them six times a second is a phone with a warm battery.
 */
export const BLOCK_POLL_MS = 1_000

/**
 * What identifies the screen on display.
 *
 * Two routes with the same key are the same screen showing new data; two with
 * different keys are a navigation. The difference decides whether the DOM is
 * repainted or rebuilt, and rebuilding is what loses focus and open state.
 */
/**
 * What the banner says, including anything held for the connection.
 *
 * Held work has to be visible. A tap that quietly waits and a tap that quietly
 * failed look identical, and somebody who cannot tell will do it again.
 */
export function waiting(queued: number, base: string): string {
  if (queued === 0) {
    return base
  }
  return `${base} ${queued} ${queued === 1 ? 'action is' : 'actions are'} waiting to send.`
}

export function mountKey(route: Route): string {
  return 'session' in route
    ? `${route.screen}:${route.session}`
    : 'interaction' in route
      ? `${route.screen}:${route.interaction}`
      : route.screen
}

/** What each surface is called on the switcher. */
const LABELS: Record<'terminal' | 'blocks' | 'transcript', string> = {
  blocks: 'Commands',
  transcript: 'Conversation',
  terminal: 'Terminal',
}

/** Which screen each surface is. */
const SCREENS: Record<'terminal' | 'blocks' | 'transcript', 'terminal' | 'blocks' | 'transcript'> =
  {
    blocks: 'blocks',
    transcript: 'transcript',
    terminal: 'terminal',
  }

/** One device that holds a credential. */
export interface DeviceRow {
  id: string
  label: string
  role: string
  revoked: boolean
  bound: boolean
}

/** One command block, as the list reports it. */
export interface BlockSummary {
  id: string
  command: string
  outcome: string
  exit_code?: number
  failed: boolean
  attributed: boolean
}

/**
 * What the exit chip says.
 *
 * A running command is not a failure and neither is one the shell ended without
 * saying how — "unknown" is genuinely different from success, and painting it
 * green would be a claim omt cannot make.
 */
export function chipLabel(block: BlockSummary): string {
  if (block.outcome === 'running') {
    return '···'
  }
  if (block.failed) {
    return block.exit_code === undefined ? 'failed' : String(block.exit_code)
  }
  if (block.outcome === 'unknown') {
    return '?'
  }
  return '✓'
}

/** Which tone that chip wears. */
export function chipTone(block: BlockSummary): string {
  if (block.outcome === 'running') {
    return 'working'
  }
  if (block.failed) {
    return 'needs-you'
  }
  return block.outcome === 'unknown' ? 'muted' : 'ok'
}

/** A session, as the roster needs it. */
export interface SessionRow {
  id: string
  title: string
  state: string
}

/** Whether this row is the reason the phone buzzed. */
export function needsYou(state: string): boolean {
  return state === 'blocked'
}

/**
 * Order the roster.
 *
 * Blocked first, and never by spawn order: that buries the one row that matters
 * behind four that do not, which on a phone means scrolling to find the only
 * thing you opened the app for. Ties break by name so the list does not shuffle
 * between glances.
 */
export function orderRoster<T extends { state: string; title: string }>(
  rows: readonly T[],
): T[] {
  const rank = (s: string) => (s === 'blocked' ? 0 : s === 'working' ? 1 : s === 'idle' ? 2 : 3)
  return [...rows].sort((a, b) => rank(a.state) - rank(b.state) || a.title.localeCompare(b.title))
}

/**
 * What the roster's bar says under the title.
 *
 * Derived from the same rows the body renders. The bug that started this
 * rebuild was a header counting one thing and a body counting another, both on
 * screen, disagreeing — with no way for a user to tell which was right.
 */
export function rosterSubtitle(rows: readonly SessionRow[]): string {
  const blocked = rows.filter((r) => needsYou(r.state)).length
  if (blocked > 0) {
    return `${blocked} of ${rows.length} need you`
  }
  return rows.length === 1 ? '1 session' : `${rows.length} sessions`
}

/**
 * Which screen a session opens.
 *
 * Decided from what the session *is*. Routing everything to the thread grid —
 * which the old client did — shows an empty screen for a plain shell, which is
 * the one thing somebody opens a terminal for.
 */
/**
 * Which screen a roster tap opens.
 *
 * By what the session *is*. An agent session opens on its conversation, because
 * that is what somebody picked the phone up to read; a shell opens on its
 * commands where the shell marks them and on the grid where it does not. The
 * grid is always one tap away and is never the first answer when something
 * better exists.
 */
export function openingScreen(
  session: { state: string },
  threads: number,
): 'transcript' | 'terminal' | 'threads' {
  if (threads > 1) {
    // More than one thread is a grid of subagents, and picking one of them for
    // somebody would be picking wrong most of the time.
    return 'threads'
  }
  if (threads === 1) {
    return 'transcript'
  }
  return 'terminal'
}

export function screenFor(
  session: { state: string },
  threads: number,
): 'terminal' | 'threads' {
  return threads > 0 || session.state === 'blocked' ? 'threads' : 'terminal'
}

/** What a card offers, and how hard each option is to choose. */
export interface CardOption {
  label: string
  confirm: 'tap' | 'hold'
  isDefault: boolean
}

/**
 * Lay out a card's options.
 *
 * The agent's own options, verbatim and in its order — omt neither adds,
 * removes nor reorders them, because the labels are how an answer is delivered.
 * What omt adds is only *how hard each one is to choose*.
 */
export function layOutCard(options: readonly string[], command: string | undefined): CardOption[] {
  const risk = command === undefined ? 'ordinary' : riskOf(command)
  return options.map((label, index) => ({
    label,
    // Only affirmative options inherit the command's risk. Making "Deny" hard
    // to press would push people toward the dangerous answer.
    confirm: isAffirmative(label) ? confirmation(risk) : 'tap',
    isDefault: index === 0,
  }))
}

function isAffirmative(label: string): boolean {
  return /^(yes|allow|approve|ok|confirm|proceed|continue)/i.test(label.trim())
}

/** Whether a card can be acted on here, and why not if not. */
export function cardOffer(card: InteractionCard): {
  answerable: boolean
  why?: string
  options: CardOption[]
} {
  if (card.deliverable === 'none') {
    return {
      answerable: false,
      why: card.not_deliverable_because ?? 'omt has no channel to answer this one',
      options: [],
    }
  }
  if (card.state !== 'open') {
    return { answerable: false, why: 'it has already been answered', options: [] }
  }
  // A permission card's prompt is the command, and the command is what decides
  // how hard "yes" is to press. Running the heuristic over a *question* would
  // fire on the word "delete" in ordinary prose.
  const command = card.kind === 'permission' ? card.prompt : undefined
  return { answerable: true, options: layOutCard(card.options, command) }
}

/** The client. */
export class App {
  #store: Store
  #root: HTMLElement
  #nav: Navigator
  /** What the terminal screen is showing. Its own, so an unrelated store
   * update cannot move it — that race is what left a spinner turning forever
   * over a session that was answering perfectly well. */
  #terminal: ScreenState<Snapshot> = { kind: 'loading' }
  /** The input the device keyboard types into. */
  #input: HTMLTextAreaElement | null = null
  #epoch = 0
  #poll: ReturnType<typeof setInterval> | null = null
  #latched: 'ctrl' | 'alt' | null = null
  #cards: InteractionCard[] | undefined
  #threads: Record<string, number> = {}
  #terminalSession: string | null = null
  /** Which session's terminal is currently mounted, so a repaint can tell
   * itself apart from a screen change. */
  #mountedSession: string | null = null
  /** Where the grid is drawn, repainted in place. */
  #grid: HTMLElement | null = null
  /** Which screen is mounted, so a repaint can tell itself from a change. */
  #mounted: string | null = null
  /** Where a list screen's rows are drawn, repainted in place. */
  #list: HTMLElement | null = null
  /** How to repaint that list. */
  #paint: (() => void) | null = null
  /** Where a refused write is explained. */
  #notice: HTMLElement | null = null
  /** What block view is showing. */
  #blocks: ScreenState<readonly BlockSummary[]> = { kind: 'loading' }
  /** What transcript view is showing. */
  #transcript: ScreenState<readonly TranscriptEntry[]> = { kind: 'loading' }
  /** Whether each session's shell marks commands, so block view can say why it
   * is empty rather than implying nothing has been run. */
  #shellIntegration: Record<string, boolean> = {}
  /** What is in the search field, kept so returning to it does not clear it. */
  #query = ''
  /** The terminal's font size, which a pinch changes. */
  #fontSize = 14
  /** How to tell the instance the grid changed size. */
  #onResize: (() => void) | null = null
  /** A half-typed command per session, so a repaint does not lose it. */
  #draft: Record<string, string> = {}
  /** Whether the key bar is folded away. It eats viewport, and on a
   * forty-column screen that matters. */
  #keysHidden = false
  /** Whether the key bar is showing the agent row. */
  #keysSecondary = false
  /** Devices this instance has let in. */
  #devices: DeviceRow[] = []
  /** Other instances whose rosters join this one. */
  #peers: { id: string; label: string; store: Store }[] = []
  /** What the agent in each session offers: its commands and its queue. */
  #offers: Record<string, { commands: { name: string; description?: string }[]; queue: string[] }> =
    {}
  /** Per-screen, so one screen's failure never appears under another's content. */
  #errors: Partial<Record<Route['screen'], string>> = {}

  #storage: Storage
  /**
   * How the client starts again against a different instance.
   *
   * Switching means a different origin, a different credential and a different
   * socket, so it is a reload rather than a rewire — there is no partial state
   * worth preserving across it, and rewiring in place is how a client ends up
   * showing one instance's sessions with another's terminal.
   */
  #restart: () => void

  constructor(
    store: Store,
    root: HTMLElement,
    nav: Navigator = new Navigator(),
    storage: Storage = window.localStorage,
    restart: () => void = () => window.location.reload(),
  ) {
    this.#store = store
    this.#root = root
    this.#nav = nav
    this.#storage = storage
    this.#restart = restart
    store.subscribe(() => this.render())
    nav.subscribe(() => this.onRouteChange())
  }

  /**
   * Follow another instance's roster.
   *
   * Read-only from here: its rows appear in the list, and opening one switches
   * to that instance first. A terminal is a connection, not a list entry, and
   * pretending otherwise would have somebody typing into a session on a machine
   * the screen is not connected to.
   */
  federate(id: string, label: string, peer: Store): void {
    this.#peers.push({ id, label, store: peer })
    peer.subscribe(() => this.render())
  }

  /** Every instance's sessions, as one list. */
  get unified(): UnifiedSession[] {
    const here = current(this.#storage)
    const rosters: InstanceRoster[] = [
      {
        instance: here?.id ?? 'local',
        label: here?.label ?? 'this instance',
        reachable: this.#store.state.connection === 'connected',
        sessions: this.#store.state.sessions as { id: string; title: string; state: string }[],
        threads: this.#threads,
      },
      ...this.#peers.map((peer) => ({
        instance: peer.id,
        label: peer.label,
        reachable: peer.store.state.connection === 'connected',
        sessions: peer.store.state.sessions as { id: string; title: string; state: string }[],
      })),
    ]
    return unify(rosters)
  }

  /** Be told when the grid needs re-measuring. */
  onResize(fit: () => void): void {
    this.#onResize = fit
  }

  /** Where we are. */
  get route(): Route {
    return this.#nav.route
  }

  /** Go somewhere. */
  go(route: Route): void {
    this.#nav.push(route)
  }

  /** Which modifier is armed, for the key bar to show. */
  get latched(): 'ctrl' | 'alt' | null {
    return this.#latched
  }

  /** A key from the accessory bar. */
  pressBarKey(index: number, secondary = false): string | null {
    const key = (secondary ? KEY_BAR_SECONDARY : KEY_BAR)[index]
    if (!key) {
      return null
    }
    if ('latch' in key.action) {
      // Tapping an armed modifier releases it — otherwise a mis-tap arms Ctrl
      // and the next letter does something unexpected.
      this.#latched = this.#latched === key.action.latch ? null : key.action.latch
      this.render()
      return null
    }
    const bytes = applyLatch(this.#latched, key.action.send)
    this.#latched = null
    this.render()
    return bytes
  }

  /** Type into the session on screen. */
  async type(text: string): Promise<void> {
    const route = this.#nav.route
    if (route.screen !== 'terminal') {
      return
    }
    try {
      await this.#store.call('session.write', route.session, text, this.#epoch)
      // Cleared on success, or the explanation for a keystroke that failed
      // once stays on screen over a terminal that is working again.
      delete this.#errors.terminal
      await this.refreshTerminal(route.session)
    } catch (e) {
      this.fail('terminal', e)
    }
  }

  /** Draw. */
  render(): void {
    const route = this.#nav.route
    // What is on screen right now. A screen whose key is unchanged is repainted
    // in place rather than rebuilt, because rebuilding throws away everything
    // the browser owns: focus, an open disclosure, the text in a field, a
    // partly-composed IME word. The terminal proved that first — the input was
    // detached six times a second — and every screen with a control in it has
    // the same problem.
    const key = mountKey(route)
    if (key === this.#mounted) {
      if (this.#list !== null) {
        // Already up: repaint just the part that carries data.
        this.#paintList()
        return
      }
      if (route.screen === 'terminal') {
        // The terminal decides for itself whether this is a repaint or a
        // rebuild: what is at the bottom of it depends on what the program is
        // doing, not only on which session is open.
        this.#renderTerminal(route.session)
        return
      }
      if (route.screen === 'search') {
        // Nothing here is driven by the store, and repainting would take the
        // field out from under whoever is typing in it.
        return
      }
    } else {
      this.#mounted = key
      this.#grid = null
      this.#list = null
      this.#paint = null
      this.#notice = null
      this.#mountedSession = null
    }
    // Cleared before every rebuild, including a repaint of a screen that has no
    // in-place painter. Skipping it appends a second copy of the screen to the
    // first, which is how a roster ends up with six title bars.
    this.#root.textContent = ''

    switch (route.screen) {
      case 'roster':
        this.#renderRoster()
        break
      case 'terminal':
        this.#renderTerminal(route.session)
        break
      case 'threads':
        this.#renderThreads(route.session)
        break
      case 'card':
        this.#renderCard(route.interaction)
        break
      case 'instances':
        this.#renderInstances()
        break
      case 'blocks':
        this.#renderBlocks(route.session)
        break
      case 'transcript':
        this.#renderTranscript(route.session)
        break
      case 'search':
        this.#renderSearch(route.session)
        break
    }
  }

  /**
   * Block view: the commands run in a session, as cards.
   *
   * The escape from a forty-column phone. Output is already segmented into
   * records by the shell's own marks, and a list of collapsible cards is a
   * native mobile shape — you scroll it with a thumb and you never pan
   * sideways, because each body is its own scroll container.
   */
  #renderBlocks(session: string): void {
    const title = this.#store.state.sessions.find((s) => s.id === session)?.title
    const bar = appBar({
      title: title ?? 'Commands',
      onBack: () => this.#nav.back(),
      action: {
        label: 'Search this session',
        onTap: () => this.go({ screen: 'search', session }),
      },
    })


    const body = el('div', 'body')
    // Said rather than shown as an empty list, which reads as "you have run
    // nothing" about a shell that has run plenty.
    const notice = el(
      'p',
      'notice',
      'This shell does not mark where commands start, so omt cannot separate them. The terminal shows everything.',
    )
    notice.hidden = true
    const list = el('div', 'block-list')
    this.#list = list
    this.#paint = () => {
      // Painted here rather than at mount, because whether the shell marks
      // commands is only known once the first list has come back — rendered
      // once at mount, the explanation was built before the answer existed and
      // never appeared.
      notice.hidden = this.#shellIntegration[session] !== false
      // Which blocks are open is the reader's state, not the data's. Rebuilding
      // the list every second would close whatever they had just opened.
      const open = new Set(
        Array.from(list.querySelectorAll('details[open]'), (node) =>
          node.getAttribute('data-block'),
        ),
      )
      list.textContent = ''
      list.append(
        renderState(this.#blocks, (blocks) =>
          blocks.map((block) => {
            const card = this.#blockCard(session, block)
            if (open.has(block.id)) {
              ;(card as HTMLDetailsElement).open = true
            }
            return card
          }),
        ),
      )
    }
    this.#paint()
    body.append(notice, list)
    this.#shell(bar, body, undefined, this.#surfaceSwitch(session, 'blocks'))
  }

  /** One command, as a card. */
  #blockCard(session: string, block: BlockSummary): HTMLElement {
    const card = el('details', block.failed ? 'block failed' : 'block')
    card.setAttribute('data-block', block.id)
    const head = el('summary')
    head.append(el('span', 'block-glyph', block.attributed ? '$' : '·'))
    head.append(el('span', 'block-command', block.command || '(output)'))
    head.append(el('span', `chip ${chipTone(block)}`, chipLabel(block)))
    card.append(head)

    const output = el('pre', 'block-output')
    card.append(output)
    card.addEventListener('toggle', () => {
      if (!card.open || output.dataset['loaded'] === 'yes') {
        return
      }
      output.dataset['loaded'] = 'yes'
      void this.#store
        .call('block.body', session, block.id)
        .then((body) => {
          const lines = (body as { lines: { text?: string }[][] }).lines
          output.textContent = lines
            .map((line) => line.map((run) => run.text ?? '').join('').trimEnd())
            .join('\n')
        })
        .catch((e: unknown) => {
          output.textContent = String(e)
        })
    })

    if (block.attributed && block.command.trim() !== '') {
      // Offered only where omt saw the command. For a block opened from output
      // alone there is nothing to run again, and the alternative is scraping
      // the screen for something that looks like a prompt.
      const again = el('button', 'block-action', 'Run again')
      again.addEventListener('click', () => void this.rerun(session, block.id))
      card.append(again)
    }
    return card
  }

  /**
   * The composer: where somebody talks to an agent.
   *
   * A textarea rather than an input, because a message to an agent is a
   * paragraph more often than a line, and a single-line field trains people to
   * send three fragments instead of one thought.
   */
  #composer(session: string): HTMLElement {
    const box = el('div', 'composer')
    const offers = this.#offers[session]

    // The queue, mirrored verbatim under a busy session. Absent rather than
    // shown empty for an agent that reports no queue at all: an empty section
    // implies the feature is there and idle.
    if (offers && offers.queue.length > 0) {
      const queue = el('div', 'queue')
      queue.append(el('p', 'entry-tag', `Queue (${offers.queue.length})`))
      offers.queue.forEach((text, index) => {
        const row = el('div', 'queue-item')
        row.append(el('span', 'meta', String(index + 1)))
        row.append(el('span', 'label', text))
        queue.append(row)
      })
      // Read-only, and it says so: omt mirrors this queue rather than owning
      // it, so a remove it cannot confirm would lose the message.
      queue.append(el('p', 'detail', 'Mirrored from the agent — omt cannot change it from here.'))
      box.append(queue)
    }

    const popup = el('div', 'commands')
    popup.hidden = true
    box.append(popup)

    const field = document.createElement('textarea')
    field.className = 'composer-field'
    field.rows = 1
    field.placeholder = 'Message'
    field.setAttribute('aria-label', 'Message the agent')
    field.setAttribute('autocapitalize', 'sentences')

    const closePopup = () => {
      popup.hidden = true
      popup.textContent = ''
    }

    field.addEventListener('input', () => {
      // Grows with what is typed, to a point: a composer that stays one line
      // hides the beginning of the message being written.
      field.style.height = 'auto'
      field.style.height = `${Math.min(field.scrollHeight, 160)}px`

      const typed = field.value
      const slash = /^\/(\S*)$/.exec(typed)
      if (slash === null || offers === undefined || offers.commands.length === 0) {
        closePopup()
        return
      }
      // A true completion popup over the agent's *own* resolved commands.
      // Never a list omt keeps: offering a command the agent does not have is
      // worse than offering none.
      const prefix = (slash[1] ?? '').toLowerCase()
      const matches = offers.commands.filter((c) => c.name.toLowerCase().startsWith(prefix))
      popup.textContent = ''
      if (matches.length === 0) {
        closePopup()
        return
      }
      for (const command of matches.slice(0, 8)) {
        const item = el('button', 'command')
        item.append(el('span', 'command-name', `/${command.name}`))
        if (command.description) {
          item.append(el('span', 'command-doc', command.description))
        }
        item.addEventListener('click', () => {
          field.value = `/${command.name} `
          closePopup()
          field.focus()
        })
        popup.append(item)
      }
      popup.hidden = false
    })

    const send = el('button', 'composer-send', 'Send')
    send.setAttribute('aria-label', 'Send the message')
    send.addEventListener('click', () => {
      const text = field.value.trim()
      if (text === '') {
        return
      }
      field.value = ''
      field.style.height = 'auto'
      closePopup()
      void this.type(`${text}\r`)
    })

    // An attachment is a file the agent should look at. It goes through the
    // ordinary chunked write, so a large one over a slow link shows progress
    // and resumes rather than failing at the end.
    const attach = el('button', 'composer-icon', '＋')
    attach.setAttribute('aria-label', 'Attach a file')
    const picker = document.createElement('input')
    picker.type = 'file'
    picker.className = 'hidden-input'
    picker.addEventListener('change', () => {
      const file = picker.files?.[0]
      if (file) {
        void this.attach(session, file)
      }
      // Cleared, or picking the same file twice fires no event the second time.
      picker.value = ''
    })
    attach.addEventListener('click', () => picker.click())

    const mic = el('button', 'composer-icon', '🎙')
    mic.setAttribute('aria-label', 'Dictate a message')
    if (!dictationAvailable()) {
      // Absent capability, said rather than a button that does nothing. Most
      // browsers other than Chrome have no recogniser at all.
      mic.disabled = true
      mic.title = 'This browser has no speech recognition'
    } else {
      mic.addEventListener('click', () => {
        void dictate(
          (text, done) => {
            // Into the field rather than straight to the agent: dictation
            // mishears, and sending before somebody can look is how a typo
            // becomes an instruction.
            field.value = text
            if (done) {
              field.focus()
            }
          },
          (chunk, done) => void this.dictated(session, chunk, done),
        )
      })
    }

    box.append(attach, picker, mic, field, send)
    return box
  }

  /** Send a file to the workspace the session is in. */
  async attach(session: string, file: File): Promise<void> {
    try {
      const workspace = this.#store.state.sessions.find((s) => s.id === session)?.workspace
      if (workspace === undefined) {
        return
      }
      const bytes = new Uint8Array(await file.arrayBuffer())
      let binary = ''
      for (const byte of bytes) {
        binary += String.fromCharCode(byte)
      }
      await this.#store.call('fs.write', workspace, file.name, btoa(binary), 0, 1)
      // Named in the message rather than attached invisibly: an agent reads a
      // path, and a file it was never told about is a file it never opens.
      void this.type(`(attached ${file.name})\r`)
    } catch (e) {
      this.fail('transcript', e)
    }
  }

  /** Hand a dictated chunk to the instance's buffer. */
  async dictated(session: string, text: string, final: boolean): Promise<void> {
    try {
      await this.#store.call('voice.append', session, text, final)
    } catch {
      // The instance may have no speech buffer for this session. The text is
      // already in the field, which is what the person can see and edit.
    }
  }

  /** Run a block's command again. */
  async rerun(session: string, block: string): Promise<void> {
    try {
      const claim = (await this.#store.call('session.acquire', session)) as { epoch: number }
      await this.#store.call('block.rerun', session, block, claim.epoch)
      this.go({ screen: 'terminal', session })
    } catch (e) {
      this.fail('blocks', e)
    }
  }

  /**
   * Transcript view: what an agent said and did.
   *
   * An agent emits no command marks at all, so the block model does not apply
   * to it — and what is available instead is better than blocks, because the
   * agent's own protocol carries whole messages and typed tool calls rather
   * than a grid of bytes.
   */
  #renderTranscript(session: string): void {
    const title = this.#store.state.sessions.find((s) => s.id === session)?.title
    const bar = appBar({
      title: title ?? 'Conversation',
      onBack: () => this.#nav.back(),
      action: {
        label: 'Search this session',
        onTap: () => this.go({ screen: 'search', session }),
      },
    })


    const body = el('div', 'body transcript')
    const list = el('div', 'transcript-list')
    this.#list = list
    this.#paint = () => {
      list.textContent = ''
      list.append(
        renderState(this.#transcript, (entries) =>
          pairCalls(entries).map((entry) => renderEntry(entry)),
        ),
      )
    }
    this.#paint()
    body.append(list)
    this.#shell(bar, body, this.#composer(session), this.#surfaceSwitch(session, 'transcript'))
  }

  /**
   * Search within a session.
   *
   * Over the instance's own index of the session rather than what happens to be
   * on screen: the answer to "where did that error go" is almost always above
   * the fold, and a client-side search of the visible grid cannot find it.
   */
  #renderSearch(session: string): void {
    const bar = appBar({ title: 'Search', onBack: () => this.#nav.back() })
    const body = el('div', 'body')

    const field = document.createElement('input')
    field.className = 'search-field'
    field.type = 'search'
    field.placeholder = 'Find in this session'
    field.setAttribute('aria-label', 'Find in this session')
    field.value = this.#query
    body.append(field)

    const results = el('div', 'search-results')
    body.append(results)

    const run = () => {
      this.#query = field.value
      if (field.value.trim() === '') {
        results.textContent = ''
        return
      }
      void this.#store
        .call('search.session', session, field.value)
        .then((found) => {
          const hits = (found as { hits: { row: number; text: string }[] }).hits
          results.textContent = ''
          if (hits.length === 0) {
            results.append(el('p', 'detail', 'Nothing matched.'))
            return
          }
          for (const hit of hits) {
            const row = el('button', 'tap search-hit')
            row.setAttribute('aria-label', `line ${hit.row}: ${hit.text}`)
            row.append(el('span', 'meta', String(hit.row)))
            row.append(el('span', 'label', hit.text))
            row.addEventListener('click', () => this.go({ screen: 'terminal', session }))
            results.append(row)
          }
        })
        .catch((e: unknown) => {
          results.textContent = ''
          results.append(el('p', 'detail', String(e)))
        })
    }
    field.addEventListener('input', run)
    this.#shell(bar, body)
    field.focus()
    if (this.#query !== '') {
      run()
    }
  }

  /**
   * Every instance this browser has been paired with.
   *
   * The client held one token in one slot, so opening a second invite link
   * silently replaced the first — and a replaced token is shown once and cannot
   * be recovered. Somebody with a laptop and a desktop had to pick one.
   */
  #renderInstances(): void {
    const bar = appBar({
      title: 'Instances',
      onBack: () => this.#nav.back(),
      action: { label: 'Add an instance', onTap: () => void this.addInstance() },
    })

    const all = list(this.#storage)
    const here = current(this.#storage)
    const state = stateOf(all, this.#errors.instances, {
      headline: 'No instances yet',
      detail: 'Run `omt web` and scan the code it prints.',
    })

    const body = el('div', 'body')

    // Two ways in, both visible. The camera was reachable only by tapping `+`
    // and hoping, which is not an affordance — and scanning is the whole point
    // of printing a code.
    const ways = el('div', 'add-instance')
    const scan = el('button', 'tap primary')
    scan.append(el('span', 'label', canScan() ? 'Scan a code' : 'Scan a code — not on this browser'))
    scan.disabled = !canScan()
    if (!canScan()) {
      // Said rather than silent: over plain HTTP there is no camera at all, and
      // that is the commonest way a phone reaches omt on a LAN.
      scan.title = 'The camera needs a secure page — reach omt over HTTPS'
    }
    scan.addEventListener('click', () => void this.addInstance())
    const paste = el('button', 'tap')
    paste.append(el('span', 'label', 'Paste a link'))
    paste.addEventListener('click', () => void this.addInstance(async () => null))
    ways.append(scan, paste)
    body.append(ways)

    body.append(
      renderState(state, (rows) =>
        rows.map((instance) =>
          row({
            title: instance.label,
            meta: instance.url,
            ...(instance.id === here?.id ? { tone: 'ok' as const } : {}),
            label:
              instance.id === here?.id
                ? `${instance.label}, the one in use`
                : `switch to ${instance.label}`,
            leadsSomewhere: instance.id !== here?.id,
            onTap: () => {
              if (instance.id === here?.id) {
                return
              }
              choose(this.#storage, instance.id)
              this.#restart()
            },
          }),
        ),
      ),
    )

    // Every device this instance has let in, and a way to take one away. The
    // credential this browser holds was minted per device precisely so a lost
    // phone is one thing to revoke — which needs somewhere to do it.
    // Rendered from state rather than from a promise writing into live DOM: a
    // store update repaints this screen, and anything a pending call was about
    // to fill has been detached by then.
    if (this.#devices.length > 0) {
      const devices = el('div', 'devices')
      devices.append(el('p', 'section', 'Devices'))
      for (const device of this.#devices) {
        const entry = el('div', device.revoked ? 'device revoked' : 'device')
        entry.append(el('span', 'label', device.label))
        entry.append(el('span', 'meta', device.revoked ? 'revoked' : device.role))
        if (!device.revoked) {
          const drop = el('button', 'device-revoke', 'Revoke')
          drop.setAttribute('aria-label', `Revoke ${device.label}`)
          drop.addEventListener('click', () => void this.revokeDevice(device.id))
          entry.append(drop)
        }
        devices.append(entry)
      }
      body.append(devices)
    }

    // Forgetting is its own control rather than a swipe: a swipe that deletes
    // a credential which cannot be recovered is a gesture people make by
    // accident on a train.
    if (here !== null) {
      const drop = el('button', 'danger', `Forget ${here.label}`)
      drop.addEventListener('click', () => {
        forget(this.#storage, here.id)
        this.#restart()
      })
      body.append(drop)
    }

    this.#shell(bar, body)
  }

  /** Read which devices this instance has let in. */
  async refreshDevices(): Promise<void> {
    try {
      const out = (await this.#store.call('device.list')) as { devices: DeviceRow[] }
      this.#devices = out.devices
      this.render()
    } catch {
      // An instance too old to know about devices, or a credential that may not
      // ask. Not a broken screen: the section simply is not there.
      this.#devices = []
    }
  }

  /** Take a device's access away. */
  async revokeDevice(device: string): Promise<void> {
    try {
      await this.#store.call('device.revoke', device)
      await this.refreshDevices()
    } catch (e) {
      this.fail('instances', e)
    }
  }

  /**
   * Pair another instance.
   *
   * The camera first, because that is the path that exists so nobody has to
   * retype 32 base62 characters. A browser without `BarcodeDetector` — which
   * today is most of them other than Chrome — falls back to pasting the same
   * link, so the feature degrades to what it replaced rather than to nothing.
   */
  async addInstance(scan: () => Promise<string | null> = scanCode): Promise<void> {
    delete this.#errors.instances
    const scanned = await scan().catch(() => null)
    const text = scanned ?? window.prompt('Paste the link `omt web` printed') ?? ''
    if (text === '') {
      this.render()
      return
    }

    const instance = readInvite(text, () => crypto.randomUUID())
    if (instance === null) {
      // Said rather than swallowed: a mis-scan and a link from somewhere else
      // look identical to somebody holding a phone at a laptop.
      this.#errors.instances = 'That is not an omt invite link.'
      this.render()
      return
    }
    remember(this.#storage, instance)
    choose(this.#storage, instance.id)
    this.#restart()
  }

  /** Load whatever the new screen needs. */
  onRouteChange(): void {
    this.stopPolling()
    const route = this.#nav.route
    // The error belonged to the screen we left. Keeping it would put a failure
    // from one screen under another's content, which is what the old client did.
    delete this.#errors[route.screen]

    if (route.screen === 'terminal') {
      // Only reset when this is a different session. Coming back to one already
      // seen shows the last screen straight away: the content is still true,
      // and a spinner over known-good content is a downgrade rather than a
      // loading state.
      if (this.#terminalSession !== route.session) {
        this.#terminal = { kind: 'loading' }
        this.#terminalSession = route.session
        // Every write is checked against the writer epoch, and the epoch is
        // per session. Claimed only on creation, opening any session somebody
        // already had left this client holding another session's epoch — so
        // every keystroke was refused and the screen simply never changed.
        void this.claimWriter(route.session)
      }
      void this.refreshTerminal(route.session)
      this.#poll = setInterval(() => void this.refreshTerminal(route.session), POLL_MS)
    }
    if (route.screen === 'threads') {
      void this.#store.loadThreads(route.session)
    }
    if (route.screen === 'blocks') {
      this.#blocks = { kind: 'loading' }
      void this.refreshBlocks(route.session)
      this.#poll = setInterval(() => void this.refreshBlocks(route.session), BLOCK_POLL_MS)
    }
    if (route.screen === 'transcript') {
      this.#transcript = { kind: 'loading' }
      void this.refreshTranscript(route.session)
      void this.refreshOffers(route.session)
      this.#poll = setInterval(() => void this.refreshTranscript(route.session), BLOCK_POLL_MS)
    }
    if (route.screen === 'card' && this.#cards === undefined) {
      void this.loadCards()
    }
    if (route.screen === 'instances') {
      void this.refreshDevices()
    }
    this.render()
  }

  /** Stop asking for a screen nobody is looking at. */
  stopPolling(): void {
    if (this.#poll !== null) {
      clearInterval(this.#poll)
      this.#poll = null
    }
  }

  /** Record a failure against the screen it happened on. */
  fail(screen: Route['screen'], error: unknown): void {
    const message =
      typeof error === 'object' && error !== null && 'message' in error
        ? String((error as { message: unknown }).message)
        : String(error)
    this.#errors[screen] = message
    this.render()
  }

  /** Whatever went wrong on this screen, if anything. */
  errorFor(screen: Route['screen']): string | undefined {
    return this.#errors[screen]
  }

  /** Fetch the screen and redraw only if it moved. */
  async refreshTerminal(session: string): Promise<void> {
    try {
      const next = (await this.#store.call('session.snapshot', session)) as Snapshot
      delete this.#errors.terminal
      const current = this.#terminal.kind === 'content' ? this.#terminal.value : null
      if (changed(current, next)) {
        this.#terminal = { kind: 'content', value: next }
        this.render()
      }
    } catch (e) {
      this.stopPolling()
      this.#terminal = {
        kind: 'error',
        headline: 'That session is not there',
        detail: String(e && typeof e === 'object' && 'message' in e ? e.message : e),
      }
      this.render()
    }
  }

  /** Re-read the cards waiting across every session. */
  async loadCards(): Promise<void> {
    try {
      const out = (await this.#store.call('interaction.list')) as {
        interactions: InteractionCard[]
      }
      this.#cards = out.interactions
      delete this.#errors.card
      this.render()
    } catch (e) {
      this.fail('card', e)
    }
  }

  /** The cards this client knows about. */
  get cards(): readonly InteractionCard[] {
    return this.#cards ?? []
  }

  /** Start a session and go straight to its terminal. */
  async newSession(workspace: string): Promise<void> {
    try {
      const out = (await this.#store.call('session.create', workspace)) as { session: string }
      const claim = (await this.#store.call('session.acquire', out.session)) as { epoch: number }
      this.#epoch = claim.epoch
      await this.#store.refresh()
      // Straight to the terminal, because the reason somebody pressed this is
      // that they want to type.
      this.go({ screen: 'terminal', session: out.session })
    } catch (e) {
      this.fail('roster', e)
    }
  }

  /**
   * Take the writer token for a session.
   *
   * Not forced: somebody else driving this session keeps it, and the terminal
   * stays readable rather than being taken out from under them.
   */
  async claimWriter(session: string): Promise<void> {
    try {
      const claim = (await this.#store.call('session.acquire', session)) as { epoch: number }
      this.#epoch = claim.epoch
    } catch (e) {
      this.fail('terminal', e)
    }
  }

  /** Fetch the blocks for block view. */
  async refreshBlocks(session: string): Promise<void> {
    try {
      const out = (await this.#store.call('session.blocks', session)) as {
        blocks: BlockSummary[]
        shell_integration: boolean
      }
      this.#shellIntegration[session] = out.shell_integration
      this.#blocks = stateOf(out.blocks, undefined, {
        headline: 'Nothing run yet',
        detail: 'Commands appear here as they are run.',
      })
      this.render()
    } catch (e) {
      this.#blocks = { kind: 'error', headline: 'That did not work', detail: String(e) }
      this.render()
    }
  }

  /** Fetch the transcript for transcript view. */
  async refreshTranscript(session: string): Promise<void> {
    try {
      const out = (await this.#store.call('session.transcript', session)) as {
        entries: TranscriptEntry[]
      }
      this.#transcript = stateOf(out.entries, undefined, {
        headline: 'Nothing said yet',
        detail: 'What the agent says and does appears here.',
      })
      this.render()
    } catch (e) {
      this.#transcript = { kind: 'error', headline: 'That did not work', detail: String(e) }
      this.render()
    }
  }

  /** Fetch what the agent offers: its commands and its queue. */
  async refreshOffers(session: string): Promise<void> {
    try {
      const out = (await this.#store.call('agent.offers', session)) as {
        commands: { name: string; description?: string }[]
        queue: string[]
      }
      this.#offers[session] = { commands: out.commands, queue: out.queue }
      this.render()
    } catch {
      // An instance too old to know about this is not a failure of the screen
      // it is on: the composer simply offers no completions.
    }
  }

  /** Answer a card. */
  async answer(interaction: string, option: string): Promise<void> {
    try {
      await this.#store.call('interaction.respond', interaction, option)
      this.#cards = undefined
      await this.#store.refresh()
      this.#nav.back()
    } catch (e) {
      // A card that failed to send must not look answered, so the screen stays.
      this.fail('card', e)
    }
  }

  /**
   * What to say about the connection, or nothing when it is fine.
   *
   * A phone loses its network constantly — a lift, a tunnel, a train. Without
   * this the screen simply stops changing, which is indistinguishable from an
   * instance where nothing is happening, and somebody acts on a stale roster.
   */
  #connectionBanner(): HTMLElement | null {
    const view = this.#store.state
    if (view.connection === 'connected') {
      return null
    }
    const box = el('div', `connection ${view.connection}`)
    box.setAttribute('role', 'status')
    box.textContent =
      view.connection === 'refused'
        ? (view.refusal ?? 'This instance refused the credential.')
        : view.connection === 'connecting'
          ? waiting(this.#store.queued, 'Reconnecting…')
          : waiting(this.#store.queued, 'Offline — showing what was last known.')
    return box
  }

  #shell(
    bar: HTMLElement,
    body: HTMLElement,
    keybar?: HTMLElement,
    under?: HTMLElement,
  ): void {
    this.#root.append(bar)
    // Under the bar, never inside it: as a full-width child of a flex row it
    // forced the bar to wrap, and a wrapped bar puts the action button on a
    // line nobody taps.
    if (under) {
      this.#root.append(under)
    }
    // Under the bar and above everything else, on every screen: a connection
    // that has gone is a fact about all of them, and saying it once per screen
    // would mean forgetting it on the screen where it matters.
    const connection = this.#connectionBanner()
    if (connection) {
      this.#root.append(connection)
    }
    this.#root.append(body)
    if (keybar) {
      this.#root.append(keybar)
    }
  }

  #renderRoster(): void {
    const view = this.#store.state
    // One list across every instance this browser knows about. With only one
    // paired it is exactly the old roster, so there is no second code path for
    // the common case to drift from.
    const all = this.unified
    const rows: SessionRow[] | undefined =
      view.connection === 'connected' || this.#peers.length > 0 ? (all as SessionRow[]) : undefined

    const state = stateOf(rows, this.#errors.roster ?? view.refusal, {
      headline: 'Nothing running',
      detail: 'Start a session and it appears here.',
    })

    const workspace = view.workspaces[0]
    const here = current(this.#storage)
    // The mark adapts: a roster with an agent waiting in it is not the same
    // screen as a roster of shells, and the icon is the cheapest place to say
    // so on a device where the whole screen is one column.
    const anyAgent = Object.values(this.#threads).some((n) => n > 0)
    const bar = appBar({
      // Named after the instance, and the name is the control: with more than
      // one paired, "omt" alone does not say whose sessions these are.
      title: here?.label ?? 'omt',
      onTitleTap: () => this.go({ screen: 'instances' }),
      mark: markFor(anyAgent ? 1 : 0),
      // From the same rows the body renders, so the two cannot disagree — and
      // adding instances is exactly where that would happen again.
      ...(rows ? { subtitle: unifiedSubtitle(all, this.#peers.length + 1) } : {}),
      ...(workspace
        ? { action: { label: 'New session', onTap: () => void this.newSession(workspace.id) } }
        : {}),
    })

    const body = el('div', 'body')
    const waiting = this.cards.filter((c) => c.state === 'open')
    if (waiting.length > 0) {
      body.append(el('p', 'section', 'Needs you'))
      for (const card of waiting) {
        body.append(
          row({
            title: card.prompt,
            tone: 'needs-you',
            label: `needs you: ${card.prompt}`,
            onTap: () => this.go({ screen: 'card', interaction: card.id }),
          }),
        )
      }
      body.append(el('p', 'section', 'Sessions'))
    }

    body.append(
      renderState(state, (sessions) =>
        sessions.map((session) =>
          row({
            title: session.title,
            meta:
              this.#peers.length > 0
                ? `${(session as unknown as UnifiedSession).instanceLabel} · ${session.state}`
                : session.state,
            ...(needsYou(session.state)
              ? { tone: 'needs-you' as const }
              : session.state === 'running'
                ? { tone: 'working' as const }
                : {}),
            label: `${session.title} — ${needsYou(session.state) ? 'needs you' : session.state}`,
            onTap: () => {
              const row = session as unknown as UnifiedSession
              const here = current(this.#storage)
              if (here && row.instance && row.instance !== here.id) {
                // On another machine. Switch to it first: a terminal is a
                // connection, and opening one against an instance this screen
                // is not connected to would type into nothing.
                choose(this.#storage, row.instance)
                this.#restart()
                return
              }
              const threads = this.#threads[session.id] ?? 0
              this.go({ screen: openingScreen(session, threads), session: session.id })
            },
          }),
        ),
      ),
    )

    this.#shell(bar, body)
  }

  /**
   * The terminal screen.
   *
   * Mounted once per session and then *repainted in place*, never rebuilt. The
   * screen repaints every 150ms, and rebuilding it each time detaches the
   * input — which blurs it, because detaching always does. The keyboard closed
   * six times a second, an IME composition could never finish, and dictation
   * stopped mid-sentence. None of it is visible on a desktop, where the
   * keyboard is not attached to the element at all.
   */
  #renderTerminal(session: string): void {
    // Mounted per session, and the mount is keyed on what the bottom of the
    // screen is: a full-screen program takes keys directly and an ordinary
    // shell is typed into through a field, and switching between them is a
    // different screen rather than a repaint.
    const direct = this.#directInput()
    const shape = `${session}:${direct ? 'direct' : 'compose'}:${this.#keysHidden}:${this.#keysSecondary}`
    if (this.#mountedSession === shape && this.#grid !== null) {
      this.#paintGrid()
      this.#paintNotice()
      return
    }
    // A different shape is a rebuild, and a rebuild starts from nothing.
    // Appending to what is already there is how one screen ends up with two key
    // bars and two of every control on them.
    this.#root.textContent = ''

    const title = this.#store.state.sessions.find((s) => s.id === session)?.title
    const bar = appBar({
      title: title ?? 'Terminal',
      onBack: () => this.#nav.back(),
      action: {
        label: 'Search this session',
        onTap: () => this.go({ screen: 'search', session }),
      },
    })

    const body = el('div', 'body terminal-body')
    const notice = el('div', 'notice')
    this.#notice = notice
    const grid = el('div', 'terminal-grid')
    this.#grid = grid
    body.append(notice, grid)
    this.#paintGrid()
    this.#paintNotice()

    this.#bindGestures(grid, session)

    const foot = el('div', 'terminal-foot')
    foot.append(this.#keybar(), this.#terminalComposer(session))

    this.#shell(bar, body, foot, this.#surfaceSwitch(session, 'terminal'))
    this.#mountedSession = shape

    // Focus goes to whichever field this screen is typed through, after it is
    // in the document — before, focus does nothing.
    const field = this.#root.querySelector('.composer-field, .hidden-input')
    if (field instanceof HTMLElement) {
      field.focus()
    }
  }

  /**
   * Say why a keystroke did nothing.
   *
   * A refused write was recorded and never shown: somebody typed, nothing
   * appeared, and there was no error anywhere to explain it. The commonest
   * reason is that another client holds the writer token, which is a fact about
   * the session and not a fault.
   */
  #paintNotice(): void {
    const notice = this.#notice
    if (notice === null) {
      return
    }
    const problem = this.#errors.terminal
    notice.textContent = problem ?? ''
    notice.hidden = problem === undefined
  }

  /**
   * Move between a session's surfaces.
   *
   * Persistent rather than hidden in a menu: each of these is a complete way of
   * reading the session, not a preview of another, and the one somebody wants
   * depends on what they came to find out.
   *
   * A surface that does not apply is shown disabled with the reason, because a
   * control that silently is not there reads as a missing feature.
   */
  #surfaceSwitch(session: string, here: 'terminal' | 'blocks' | 'transcript'): HTMLElement {
    const threads = this.#threads[session] ?? 0
    const offers = surfacesFor({
      threads,
      shellIntegration: this.#shellIntegration[session] ?? true,
      native: false,
    })

    const bar = el('div', 'surfaces')
    bar.setAttribute('role', 'tablist')
    for (const offer of offers) {
      const tab = el('button', 'surface', LABELS[offer.surface])
      tab.setAttribute('role', 'tab')
      tab.setAttribute('aria-selected', String(offer.surface === here))
      if (!offer.available) {
        tab.disabled = true
        // The reason, on the control itself: a disabled tab with no
        // explanation is indistinguishable from a broken one.
        tab.title = offer.why ?? ''
        tab.setAttribute('aria-label', `${LABELS[offer.surface]} — ${offer.why ?? 'unavailable'}`)
      } else if (offer.surface !== here) {
        tab.addEventListener('click', () =>
          this.go({ screen: SCREENS[offer.surface], session }),
        )
      }
      bar.append(tab)
    }
    return bar
  }

  /**
   * The gestures that replace keys a phone does not have.
   *
   * `24 §3.2`. Reaching the key bar for every cursor movement is what makes a
   * mobile terminal feel like a toy, and long-press-and-drag for arrows is the
   * gesture that fixes it.
   *
   * Nothing destructive is here. A movement somebody makes by accident must not
   * be able to stop an agent.
   */
  #bindGestures(grid: HTMLElement, session: string): void {
    let holdTimer: ReturnType<typeof setTimeout> | null = null
    let arrowing = false
    let from = { x: 0, y: 0 }
    let sent = 0
    let lastTap: { at: number; x: number; y: number } | null = null
    let pinch: { span: number; font: number } | null = null

    const stop = () => {
      if (holdTimer !== null) {
        clearTimeout(holdTimer)
        holdTimer = null
      }
      arrowing = false
      sent = 0
      grid.classList.remove('arrowing')
    }

    grid.addEventListener(
      'touchstart',
      (event) => {
        if (event.touches.length === 2) {
          // A pinch resizes the grid rather than magnifying it: a zoomed
          // terminal is a blurry one you then pan around.
          stop()
          pinch = { span: spanOf(Array.from(event.touches)), font: this.#fontSize }
          return
        }
        const touch = event.touches[0]
        if (touch === undefined) {
          return
        }
        from = { x: touch.clientX, y: touch.clientY }

        const now = Date.now()
        if (lastTap && isDoubleTap(lastTap, { at: now, x: from.x, y: from.y })) {
          // Completion without reaching for the key bar.
          lastTap = null
          event.preventDefault()
          void this.type('\t')
          return
        }
        lastTap = { at: now, x: from.x, y: from.y }

        holdTimer = setTimeout(() => {
          arrowing = true
          sent = 0
          // Said with the cursor, because a mode nobody can see is a mode
          // somebody is surprised by.
          grid.classList.add('arrowing')
        }, DRAG_ARM_MS)
      },
      { passive: false },
    )

    grid.addEventListener(
      'touchmove',
      (event) => {
        if (pinch && event.touches.length === 2) {
          event.preventDefault()
          const wanted = fontForPinch(pinch.font, pinch.span, spanOf(Array.from(event.touches)))
          if (wanted !== this.#fontSize) {
            this.#fontSize = wanted
            document.documentElement.style.setProperty('--terminal-font', `${wanted}px`)
            // The grid follows the font: fewer columns at a larger size, and a
            // SIGWINCH so the program redraws to fit.
            this.#onResize?.()
          }
          return
        }
        const touch = event.touches[0]
        if (touch === undefined) {
          return
        }
        if (!arrowing) {
          // Moving before the hold completed is a scroll, not an arrow.
          const moved = Math.hypot(touch.clientX - from.x, touch.clientY - from.y)
          if (moved > 12 && holdTimer !== null) {
            clearTimeout(holdTimer)
            holdTimer = null
          }
          return
        }

        event.preventDefault()
        const dx = touch.clientX - from.x
        const dy = touch.clientY - from.y
        const direction = directionOf(dx, dy)
        if (direction === null) {
          return
        }
        const wanted = repeatsFor(direction === 'left' || direction === 'right' ? dx : dy)
        // Only the difference: a drag that has already sent four keys and grows
        // to six sends two more, not six more.
        for (let i = sent; i < wanted; i += 1) {
          void this.type(`\x1b${ARROWS[direction]}`)
        }
        sent = Math.max(sent, wanted)
      },
      { passive: false },
    )

    for (const ending of ['touchend', 'touchcancel']) {
      grid.addEventListener(ending, () => {
        pinch = null
        stop()
      })
    }
  }

  /** Redraw whichever list screen is mounted. */
  #paintList(): void {
    this.#paint?.()
  }

  /** Redraw the grid from whatever the terminal screen is showing. */
  #paintGrid(): void {
    const grid = this.#grid
    if (grid === null) {
      return
    }
    grid.textContent = ''
    grid.append(
      renderState(this.#terminal, (snapshot) => {
        const host = el('div', 'terminal')
        paint(host, snapshot)
        return host
      }),
    )
  }

  /**
   * The one input the terminal screen types through.
   *
   * Built once and reused, never rebuilt per render. The terminal repaints
   * every 150ms, and a textarea replaced on every repaint is a keyboard that
   * closes six times a second, an IME composition that can never finish, and
   * dictation that stops mid-sentence — none of which a desktop shows, because
   * a desktop keyboard is not attached to the element.
   */
  #terminalInput(): HTMLTextAreaElement {
    const existing = this.#input
    if (existing !== null) {
      return existing
    }
    // The device's own keyboard, reached the only way a browser opens one: a
    // real focused form control. A `keydown` listener on the document works on
    // a desktop, which is exactly why verifying there missed this entirely.
    const input = document.createElement('textarea')
    input.className = 'hidden-input'
    input.setAttribute('aria-label', 'Terminal input')
    // A terminal is not prose. Autocapitalise turns `ls` into `Ls`, which is a
    // command that does not exist, and autocorrect rewrites what is already in
    // the box so what gets sent is not what was typed.
    input.setAttribute('autocapitalize', 'off')
    input.setAttribute('autocorrect', 'off')
    input.setAttribute('autocomplete', 'off')
    input.setAttribute('spellcheck', 'false')
    // Whether an input method is mid-word.
    //
    // This is the whole of typing on a phone that is not set to English. A
    // pinyin keyboard puts `n i h a o` in the field and waits for a character
    // to be chosen; clearing the field on every `input` event — which is what
    // this did — wipes the candidate buffer on the first keystroke, so nothing
    // is ever committed and the terminal stays empty. Gboard's autocorrect uses
    // the same mechanism, so English was quietly damaged too.
    //
    // While composing, the field belongs to the input method. omt takes it back
    // at `compositionend`, which is the moment a word actually exists.
    let composing = false
    input.addEventListener('compositionstart', () => {
      composing = true
    })
    input.addEventListener('compositionend', () => {
      composing = false
      const text = input.value
      input.value = ''
      if (text.length > 0) {
        void this.type(text)
      }
    })

    input.addEventListener('input', (event) => {
      // `isComposing` as well as the flag: Android fires `input` for the
      // composing keystroke before `compositionstart` arrives, and reading only
      // the flag loses the first letter of every word.
      if (composing || (event as InputEvent).isComposing) {
        return
      }
      const text = input.value
      // Emptied every time. Left to accumulate, the second keystroke sends both
      // characters.
      input.value = ''
      if (text.length > 0) {
        void this.type(text)
      }
    })
    input.addEventListener('keydown', (event) => {
      // 229 is the keycode a browser reports for "the input method is handling
      // this". Acting on it sends a stray byte for every letter of a word being
      // composed.
      if (composing || event.isComposing || event.keyCode === 229) {
        return
      }
      const bytes = keyToBytes(event)
      if (bytes !== null) {
        event.preventDefault()
        void this.type(bytes)
      }
    })
    this.#input = input
    return input
  }

  /**
   * The keys a phone keyboard does not have.
   *
   * Eleven of them, and every one has to be reachable. Crammed into one
   * scrolling row — which is what this was — the last four are invisible, and
   * an invisible key is one nobody knows exists. They wrap instead, and the
   * second row of agent keys has a control rather than living behind a swipe
   * nobody discovers.
   */
  #keybar(): HTMLElement {
    const wrap = el('div', 'keybar-wrap')
    if (this.#keysHidden) {
      // Hideable, because it eats viewport and on a forty-column screen that
      // matters. Shown as a thin handle rather than vanishing, or there is no
      // way back.
      const show = el('button', 'keybar-handle', '⌨ keys')
      show.setAttribute('aria-label', 'Show the key bar')
      show.addEventListener('click', () => {
        this.#keysHidden = false
        this.render()
      })
      wrap.append(show)
      return wrap
    }

    const keys = this.#keysSecondary ? KEY_BAR_SECONDARY : KEY_BAR
    const bar = el('div', 'keybar')
    keys.forEach((key, index) => {
      const button = el('button', undefined, key.label)
      button.setAttribute('aria-label', key.label)
      if ('latch' in key.action) {
        button.setAttribute('aria-pressed', String(this.#latched === key.action.latch))
      }
      button.addEventListener('click', () => {
        const bytes = this.pressBarKey(index, this.#keysSecondary)
        if (bytes !== null) {
          void this.type(bytes)
        }
      })
      bar.append(button)
    })

    const controls = el('div', 'keybar-controls')
    const swap = el('button', 'keybar-control', this.#keysSecondary ? 'abc' : '^C…')
    swap.setAttribute(
      'aria-label',
      this.#keysSecondary ? 'Show the ordinary keys' : 'Show the agent keys',
    )
    swap.addEventListener('click', () => {
      this.#keysSecondary = !this.#keysSecondary
      this.render()
    })
    const hide = el('button', 'keybar-control', '⌄')
    hide.setAttribute('aria-label', 'Hide the key bar')
    hide.addEventListener('click', () => {
      this.#keysHidden = true
      this.render()
    })
    controls.append(swap, hide)

    wrap.append(bar, controls)
    return wrap
  }

  /**
   * Where somebody types on a phone.
   *
   * A real, visible field. The capture field this replaces was one pixel square
   * and transparent, which is a shape an input method cannot work with — the
   * keyboard opened, the candidates went nowhere, and nothing arrived. And per
   * `24 §3.1` drafting belongs in a text field anyway: putting a
   * paragraph-long prompt into a pty one keystroke at a time is miserable.
   *
   * A full-screen program is the exception. In the alternate screen every
   * keystroke means something immediately — `j` moves in vim — so a field you
   * submit is useless there, and omt switches to sending keys directly. It
   * knows which it is looking at, so nobody has to tell it.
   */
  #terminalComposer(session: string): HTMLElement {
    const box = el('div', 'composer')
    const direct = this.#directInput()

    if (direct) {
      const note = el('span', 'composer-note', 'Full-screen program — keys go straight through')
      const field = this.#terminalInput()
      box.append(note, field)
      return box
    }

    const field = document.createElement('textarea')
    field.className = 'composer-field'
    field.rows = 1
    field.placeholder = 'Type a command'
    field.setAttribute('aria-label', 'Type a command')
    field.setAttribute('autocapitalize', 'off')
    field.setAttribute('autocorrect', 'off')
    field.setAttribute('spellcheck', 'false')
    field.value = this.#draft[session] ?? ''

    const send = () => {
      const text = field.value
      if (text === '') {
        return
      }
      field.value = ''
      delete this.#draft[session]
      field.style.height = 'auto'
      void this.type(`${text}\r`)
    }

    field.addEventListener('input', () => {
      // Kept, so a repaint does not lose a half-typed command.
      this.#draft[session] = field.value
      field.style.height = 'auto'
      field.style.height = `${Math.min(field.scrollHeight, 140)}px`
    })
    field.addEventListener('keydown', (event) => {
      // Enter sends; shift-enter is a newline, for the paragraph this exists
      // for. Never while an input method is composing — there Enter chooses a
      // candidate and sending would cut the word in half.
      if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
        event.preventDefault()
        send()
      }
    })

    const button = el('button', 'composer-send', 'Send')
    button.setAttribute('aria-label', 'Send')
    button.addEventListener('click', send)

    box.append(field, button)
    return box
  }

  /** Whether keys should go straight through rather than through a field. */
  #directInput(): boolean {
    return this.#terminal.kind === 'content' && this.#terminal.value.alternate_screen
  }

  #renderThreads(session: string): void {
    const title = this.#store.state.sessions.find((s) => s.id === session)?.title
    const bar = appBar({
      title: title ?? 'Agents',
      onBack: () => this.#nav.back(),
    })

    const cells = this.#store.gridFor(session)
    this.#threads[session] = cells.length
    const state = stateOf(cells, this.#errors.threads, {
      headline: 'No agent here',
      detail: 'This session is a plain shell.',
    })

    const body = el('div', 'body')
    body.append(
      renderState(state, (grid) => {
        const wrapper = el('div', 'grid')
        for (const cell of grid) {
          const button = el('button', `cell ${cell.tone}`, cell.glyph)
          button.setAttribute('aria-label', cell.label)
          const first = cell.thread.open_interactions[0]
          if (first !== undefined) {
            button.addEventListener('click', () =>
              this.go({ screen: 'card', interaction: first }),
            )
          } else {
            button.disabled = true
          }
          wrapper.append(button)
        }
        return wrapper
      }),
    )

    // A plain shell has no threads, and the way to what it does have is here.
    const open = el('button', 'tap primary')
    open.append(el('span', 'label', 'Open the terminal'))
    open.addEventListener('click', () => this.go({ screen: 'terminal', session }))
    body.append(open)

    this.#shell(bar, body)
  }

  #renderCard(id: string): void {
    const card = this.#cards?.find((c) => c.id === id)
    const bar = appBar({
      title: cardTitle(card?.kind ?? 'choice'),
      onBack: () => this.#nav.back(),
    })
    const body = el('div', 'body')

    if (this.#errors.card !== undefined) {
      body.append(
        renderState(
          { kind: 'error', headline: 'That did not send', detail: this.#errors.card },
          () => el('div'),
        ),
      )
      this.#shell(bar, body)
      return
    }
    if (this.#cards === undefined) {
      body.append(renderState({ kind: 'loading' }, () => el('div')))
      this.#shell(bar, body)
      return
    }
    if (card === undefined) {
      body.append(
        renderState(
          {
            kind: 'empty',
            headline: 'Already answered',
            detail: 'Somebody got to this one first.',
          },
          () => el('div'),
        ),
      )
      this.#shell(bar, body)
      return
    }

    body.append(promptElement(card.kind, card.prompt))

    // A plan is the thing being reviewed, so it is the body of the card rather
    // than something to go and find.
    const steps = (card as { steps?: { title?: string; state?: string }[] }).steps
    if (card.kind === 'plan_review' && steps) {
      body.append(planElement(steps))
    }

    const offer = cardOffer(card)
    if (!offer.answerable) {
      body.append(
        el('p', 'banner', `omt cannot answer this here: ${offer.why ?? 'unknown'}`),
      )
      const open = el('button', 'tap primary')
      open.append(el('span', 'label', 'Open the terminal'))
      open.addEventListener('click', () =>
        this.go({ screen: 'terminal', session: card.session }),
      )
      body.append(open)
      this.#shell(bar, body)
      return
    }

    // Free text: the agent asked something no list of options answers, so the
    // card is a field rather than a row of buttons.
    if (card.kind === 'text') {
      const field = document.createElement('textarea')
      field.className = 'composer-field'
      field.rows = 3
      field.setAttribute('aria-label', 'Your answer')
      const send = el('button', 'tap primary')
      send.append(el('span', 'label', 'Send'))
      send.addEventListener('click', () => {
        const text = field.value.trim()
        if (text !== '') {
          void this.answer(card.id, text)
        }
      })
      body.append(field, send)
      this.#shell(bar, body)
      field.focus()
      return
    }

    for (const option of offer.options) {
      const button = el('button', option.confirm === 'hold' ? 'tap hold' : 'tap')
      button.append(el('span', 'label', option.label))
      // Said out loud: the difference between a tap and a hold is invisible
      // until somebody has already failed to press it.
      button.setAttribute(
        'aria-label',
        option.confirm === 'hold' ? `${option.label} — press and hold` : option.label,
      )
      if (option.confirm === 'hold') {
        // A real gesture, not a class. This rendered a different-looking button
        // that a single tap still activated, so an irreversible approval was one
        // accidental thumb away.
        holdToConfirm(button, () => void this.answer(card.id, option.label))
      } else {
        button.addEventListener('click', () => void this.answer(card.id, option.label))
      }
      body.append(button)
    }

    this.#shell(bar, body)
  }
}

export { hasBack, titleOf }
export type { Route }

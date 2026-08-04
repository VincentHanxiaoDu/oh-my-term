/**
 * The client's whole view of an instance, in one place.
 *
 * Everything a surface renders comes from here, and everything a surface does
 * goes through `call`. That is not architecture for its own sake: it means the
 * rules that matter — a position advances only for delivered events, an
 * answering affordance comes from `deliverable` — are enforced once rather
 * than in every component that happens to render a card.
 */

import { HANDLERS, type SessionSummary, type ThreadSummary, type WorkspaceSummary } from './capabilities.js'
import type { ClientMessage, Interaction, RequestId, ServerMessage } from './protocol.js'
import { isAnswerable, isTerminal } from './protocol.js'
import { InstanceClient } from './session.js'
import { type Thread, grid, summarize } from './threads.js'

/** What a surface renders. */
export interface ViewState {
  connection: 'disconnected' | 'connecting' | 'connected' | 'refused'
  /** Why, when refused. */
  refusal?: string
  workspaces: WorkspaceSummary[]
  sessions: SessionSummary[]
  /** Threads per session. */
  threads: Record<string, ThreadSummary[]>
  /** Cards still waiting, across every session. */
  open: Interaction[]
  /** How many need a human — the one number the header leads with. */
  needsYou: number
  /** Anything that went wrong, newest first. */
  problems: Problem[]
}

/** Something the user should be told. */
export interface Problem {
  /** What happened, in words they can act on. */
  message: string
  /** Whether it will resolve itself. */
  transient: boolean
}

/** A call that failed. */
export interface CallFailure {
  capability: string
  code: string
  message: string
}

/** How the store reaches the server. */
export interface Sink {
  send(message: ClientMessage): void
}

/**
 * The client's state, and the only way to change it.
 */
export class Store {
  #client: InstanceClient
  #sink: Sink
  #workspaces: WorkspaceSummary[] = []
  #sessions: SessionSummary[] = []
  #threads = new Map<string, ThreadSummary[]>()
  #problems: Problem[] = []
  #pending = new Map<string, { capability: string; resolve: (v: unknown) => void; reject: (e: CallFailure) => void }>()
  #listeners = new Set<(state: ViewState) => void>()

  constructor(device: string, sink: Sink) {
    this.#client = new InstanceClient(device)
    this.#sink = sink
  }

  /** What a surface renders right now. */
  get state(): ViewState {
    const connection = this.#client.connection
    const open = this.#client.openInteractions()
    return {
      connection: connection.state,
      ...(connection.state === 'refused' ? { refusal: connection.detail } : {}),
      workspaces: this.#workspaces,
      sessions: this.#sessions,
      threads: Object.fromEntries(this.#threads),
      open,
      needsYou: open.filter((i) => isAnswerable(i.deliverable)).length,
      problems: this.#problems,
    }
  }

  /** Be told when anything changes. */
  subscribe(listener: (state: ViewState) => void): () => void {
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }

  /** Open the connection. */
  connect(token?: string): void {
    for (const message of this.#client.hello(token)) {
      this.#sink.send(message)
    }
    this.#emit()
  }

  /**
   * Invoke a capability.
   *
   * Rejects if this instance does not offer it, *before* sending — a call that
   * went out and came back "unknown capability" costs a round trip to learn
   * something the welcome already said.
   */
  call<K extends keyof typeof HANDLERS>(
    capability: K,
    ...args: DropFirst<Parameters<(typeof HANDLERS)[K]>>
  ): Promise<unknown> {
    if (!this.#client.can(capability)) {
      return Promise.reject({
        capability,
        code: 'unsupported',
        message: `this instance does not offer ${capability}`,
      } satisfies CallFailure)
    }
    const request = this.#client.request()
    const build = HANDLERS[capability] as (r: RequestId, ...rest: unknown[]) => ClientMessage
    const message = build(request, ...args)
    return new Promise((resolve, reject) => {
      this.#pending.set(keyOf(request), { capability, resolve, reject })
      this.#sink.send(message)
    })
  }

  /** Apply a message from the server. */
  receive(message: ServerMessage): void {
    const outcome = this.#client.receive(message)

    if (message.t === 'result') {
      this.#settle(message)
    }
    if (message.t === 'welcome') {
      // The welcome says what this instance offers, not what it holds. Without
      // this the client sits on an empty roster forever, looking exactly like
      // an instance with nothing running.
      void this.refresh()
    }
    if (message.t === 'goodbye') {
      this.#problems.unshift({
        message: message.detail,
        // A refused credential will not fix itself; anything else might.
        transient: message.reason !== 'unauthorized',
      })
    }
    if (outcome === 'gap' || outcome === 'resync-needed') {
      // Said out loud rather than papered over: a client that quietly
      // continued would be showing stale state as if it were current.
      this.#problems.unshift({
        message: 'missed some updates — refreshing',
        transient: true,
      })
      void this.refresh()
    }
    this.#emit()
  }

  /**
   * Re-read everything from the instance.
   *
   * Both calls go out together. Chaining them costs two round trips before the
   * roster can draw anything, and neither answer depends on the other — over a
   * phone link that is the difference between opening to a list and opening to
   * a blank screen that fills in twice.
   */
  async refresh(): Promise<void> {
    try {
      const [workspaces, sessions] = (await Promise.all([
        this.call('workspace.list'),
        this.call('session.list'),
      ])) as [{ workspaces: WorkspaceSummary[] }, { sessions: SessionSummary[] }]
      this.#workspaces = workspaces.workspaces
      this.#sessions = sessions.sessions
    } catch (e) {
      const failure = e as CallFailure
      this.#problems.unshift({ message: failure.message, transient: true })
    }
    this.#emit()
  }

  /** Read one session's threads. */
  async loadThreads(session: string): Promise<void> {
    try {
      const out = (await this.call('agent.threads', session)) as { threads: ThreadSummary[] }
      this.#threads.set(session, out.threads)
    } catch (e) {
      this.#problems.unshift({ message: (e as CallFailure).message, transient: true })
    }
    this.#emit()
  }

  /** The subagent grid for a session, ordered for rendering. */
  gridFor(session: string): ReturnType<typeof grid> {
    return grid(toThreads(this.#threads.get(session) ?? []))
  }

  /** The header line above that grid. */
  summaryFor(session: string): string {
    return summarize(toThreads(this.#threads.get(session) ?? []))
  }

  /** Cards this client may actually answer. */
  answerable(): Interaction[] {
    return this.#client.answerable()
  }

  /** Forget a problem the user has seen. */
  dismiss(index: number): void {
    this.#problems.splice(index, 1)
    this.#emit()
  }

  #settle(message: Extract<ServerMessage, { t: 'result' }>): void {
    const key = keyOf(message.request as RequestId)
    const pending = this.#pending.get(key)
    if (!pending) {
      // A result for a request this client never made, or made twice. Acting on
      // it would apply somebody else's answer.
      return
    }
    this.#pending.delete(key)
    if (message.status === 'ok') {
      pending.resolve(message.output)
    } else {
      pending.reject({
        capability: pending.capability,
        code: message.error.code,
        message: message.error.message,
      })
    }
  }

  #emit(): void {
    const state = this.state
    for (const listener of this.#listeners) {
      listener(state)
    }
  }
}

/** Wire summaries into the shape the grid renders. */
function toThreads(summaries: ThreadSummary[]): Thread[] {
  return summaries.map((t) => ({
    id: t.id,
    is_subagent: t.is_subagent,
    state: stateOf(t.state),
    ...(t.label === undefined ? {} : { label: t.label }),
    open_interactions: t.open_interactions,
  }))
}

function stateOf(name: string): Thread['state'] {
  switch (name) {
    case 'blocked':
      return { state: 'blocked', reason: 'unspecified' }
    case 'working':
      return { state: 'working' }
    case 'idle':
      return { state: 'idle' }
    case 'exited':
      return { state: 'exited' }
    default:
      return { state: 'unknown' }
  }
}

function keyOf(request: RequestId): string {
  return `${request.device}:${request.n}`
}

/** Everything after the request id, which the store supplies. */
type DropFirst<T extends readonly unknown[]> = T extends readonly [unknown, ...infer Rest]
  ? Rest
  : never

export { isAnswerable, isTerminal }

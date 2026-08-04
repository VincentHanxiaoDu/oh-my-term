/**
 * Typed calls for each capability this client handles.
 *
 * The list in `generated/handlers.json` is what the parity gate checks, and it
 * must mean "the web client implements this" rather than "somebody remembered
 * to add a line". So every name there has a function here, and a test asserts
 * the two agree — otherwise the gate degrades into a list that passes itself.
 */

import type { ClientMessage, RequestId } from './protocol.js'

/** A summary of one workspace. */
export interface WorkspaceSummary {
  id: string
  root: string
  name: string
  sessions: number
}

/** A summary of one session. */
export interface SessionSummary {
  id: string
  workspace: string
  title: string
  state: string
  cwd?: string
}

/** One thread — the session's own, or a subagent. */
export interface ThreadSummary {
  id: string
  is_subagent: boolean
  state: string
  label?: string
  open_interactions: string[]
}

/** Build the call for a capability. */
function call(request: RequestId, capability: string, input: unknown): ClientMessage {
  return { t: 'call', request, capability, input }
}

/** Every capability this client can invoke, by name. */
export const HANDLERS = {
  'instance.info': (request: RequestId) => call(request, 'instance.info', {}),

  'instance.catalog': (request: RequestId) => call(request, 'instance.catalog', {}),

  'events.subscribe': (request: RequestId, since: Record<string, number> = {}) =>
    call(request, 'events.subscribe', { since }),

  'workspace.list': (request: RequestId) => call(request, 'workspace.list', {}),

  'workspace.open': (request: RequestId, root: string) =>
    call(request, 'workspace.open', { root }),

  'session.list': (request: RequestId, workspace?: string) =>
    call(request, 'session.list', workspace === undefined ? {} : { workspace }),

  'session.close': (request: RequestId, session: string) =>
    call(request, 'session.close', { session }),

  'agent.threads': (request: RequestId, session: string) =>
    call(request, 'agent.threads', { session }),
} as const

/** The capability names this client handles. */
export type HandledCapability = keyof typeof HANDLERS

/** Whether this client can invoke a capability. */
export function handles(capability: string): capability is HandledCapability {
  return capability in HANDLERS
}

/** Every capability this client handles, sorted. */
export function handledCapabilities(): string[] {
  return Object.keys(HANDLERS).sort()
}

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

/** One entry in a file listing. */
export interface FsEntry {
  name: string
  rel: string
  is_dir: boolean
  is_symlink: boolean
  size?: number
}

/** What git says about a workspace. */
export interface GitStatus {
  branch?: string
  ahead: number
  behind: number
  modified: number
  staged: number
  untracked: number
  dirty: boolean
}

/** One changed file. */
export interface DiffFile {
  path: string
  from?: string
  kind: string
  added: number
  removed: number
  binary: boolean
}

/** What is on a session's screen. */
export interface ScreenContents {
  screen: string[]
  history: string[]
  cursor: [number, number]
  /** While true a full-screen program owns every cell and a line view is nonsense. */
  alternate_screen: boolean
}

/** One resolved setting, and where it came from. */
export interface ConfigValue {
  key: string
  value: unknown
  layer: string
  file?: string
}

/** One line of the keyboard reference. */
export interface KeyBinding {
  chord: string
  mode: string
  action: string
  /** Whether the program underneath sees this key. */
  reaches_program: boolean
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

  'agent.interrupt': (request: RequestId, session: string) =>
    call(request, 'agent.interrupt', { session }),

  'fs.list': (request: RequestId, workspace: string, path = '') =>
    call(request, 'fs.list', { workspace, path }),

  'git.status': (request: RequestId, workspace: string) =>
    call(request, 'git.status', { workspace }),

  'git.diff': (request: RequestId, workspace: string, staged = false) =>
    call(request, 'git.diff', { workspace, staged }),

  // The epoch is required, not optional: input already in flight when the
  // writer token changed hands must be rejected rather than landing in
  // somebody else's command line.
  'session.write': (request: RequestId, session: string, text: string, epoch: number) =>
    call(request, 'session.write', { session, text, epoch }),

  'session.resize': (request: RequestId, session: string, cols: number, rows: number) =>
    call(request, 'session.resize', { session, cols, rows }),

  'session.read': (request: RequestId, session: string, history = 0) =>
    call(request, 'session.read', { session, history }),

  'session.snapshot': (request: RequestId, session: string) =>
    call(request, 'session.snapshot', { session }),

  'config.get': (request: RequestId, key?: string) =>
    call(request, 'config.get', key === undefined ? {} : { key }),

  'keys.cheatsheet': (request: RequestId) => call(request, 'keys.cheatsheet', {}),
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

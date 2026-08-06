/**
 * Typed calls for each capability this client handles.
 *
 * The list in `generated/handlers.json` is what the parity gate checks, and it
 * must mean "the web client implements this" rather than "somebody remembered
 * to add a line". So every name there has a function here, and a test asserts
 * the two agree — otherwise the gate degrades into a list that passes itself.
 */

import type { ClientMessage, RequestId } from './protocol.js'
import { CAPABILITY_INFO } from './generated/catalog.js'
import type { Capability } from './generated/catalog.js'

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
  // A command carries an intent id and a query must not. Minted here rather
  // than by each caller, because the rule is "every command, always" and a
  // rule enforced at seventeen call sites is a rule with a hole in it.
  return isCommand(capability)
    ? { t: 'call', request, capability, input, intent: mintIntent() }
    : { t: 'call', request, capability, input }
}

/** Whether a capability mutates, according to the catalog the server generated. */
export function isCommand(capability: string): boolean {
  return CAPABILITY_INFO[capability as Capability]?.kind === 'command'
}

/**
 * A fresh intent id.
 *
 * Minted before the message goes out — at intent time, not on arrival —
 * because the whole point is that a client which lost its connection can
 * repeat the identical call and be recognised rather than acted on twice.
 */
function mintIntent(): string {
  return crypto.randomUUID()
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

  'session.create': (
    request: RequestId,
    workspace: string,
    program?: string,
    cols = 80,
    rows = 24,
  ) =>
    call(request, 'session.create', {
      workspace,
      ...(program === undefined ? {} : { program }),
      cols,
      rows,
    }),

  'pane.list': (request: RequestId, workspace: string) =>
    call(request, 'pane.list', { workspace }),

  'pane.open': (request: RequestId, workspace: string, session: string) =>
    call(request, 'pane.open', { workspace, session }),

  'pane.close': (request: RequestId, workspace: string, pane: string) =>
    call(request, 'pane.close', { workspace, pane }),

  'pane.focus': (request: RequestId, workspace: string, pane: string) =>
    call(request, 'pane.focus', { workspace, pane }),

  'state.save': (request: RequestId, path?: string) =>
    call(request, 'state.save', path === undefined ? {} : { path }),

  'state.restore': (request: RequestId, path?: string) =>
    call(request, 'state.restore', path === undefined ? {} : { path }),

  'recall.suggest': (request: RequestId, prefix: string, workspace: string, limit = 10) =>
    call(request, 'recall.suggest', { prefix, workspace, limit }),

  'recall.record': (
    request: RequestId,
    command: string,
    workspace: string,
    session: string,
    exitCode?: number,
  ) =>
    call(request, 'recall.record', {
      command,
      workspace,
      session,
      ...(exitCode === undefined ? {} : { exit_code: exitCode }),
    }),

  'plugin.install': (
    request: RequestId,
    id: string,
    name: string,
    version: string,
    permissions: string[],
    grant: string[],
    entry: string[] = [],
  ) => call(request, 'plugin.install', { id, name, version, permissions, grant, entry }),

  'plugin.start': (request: RequestId, id: string) =>
    call(request, 'plugin.start', { id }),

  'plugin.list': (request: RequestId) => call(request, 'plugin.list', {}),

  'plugin.enable': (request: RequestId, id: string, enabled: boolean) =>
    call(request, 'plugin.enable', { id, enabled }),

  'job.create': (
    request: RequestId,
    name: string,
    workspace: string,
    run: string,
    everySeconds = 0,
  ) => call(request, 'job.create', { name, workspace, run, every_seconds: everySeconds }),

  'job.list': (request: RequestId) => call(request, 'job.list', {}),

  'voice.append': (request: RequestId, session: string, text: string, finalChunk = false) =>
    call(request, 'voice.append', { session, text, final_chunk: finalChunk }),

  'voice.clear': (request: RequestId, session: string) =>
    call(request, 'voice.clear', { session }),

  'theme.get': (request: RequestId) => call(request, 'theme.get', {}),

  'open.recognize': (request: RequestId, line: string) =>
    call(request, 'open.recognize', { line }),

  'fs.read': (request: RequestId, workspace: string, path: string, chunk = 0) =>
    call(request, 'fs.read', { workspace, path, chunk }),

  'fs.write': (
    request: RequestId,
    workspace: string,
    path: string,
    data: string,
    chunk = 0,
    chunks = 1,
  ) => call(request, 'fs.write', { workspace, path, data, chunk, chunks }),

  'interaction.list': (request: RequestId, session?: string) =>
    call(request, 'interaction.list', session === undefined ? {} : { session }),

  'interaction.respond': (request: RequestId, interaction: string, option: string, text?: string) =>
    call(request, 'interaction.respond', {
      interaction,
      option,
      ...(text === undefined ? {} : { text }),
    }),

  'session.acquire': (request: RequestId, session: string, force = false) =>
    call(request, 'session.acquire', { session, force }),

  'session.release': (request: RequestId, session: string) =>
    call(request, 'session.release', { session }),

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

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
  /** How many agent threads are in it, and how many need a human.
   *
   * Carried on the roster itself rather than fetched per session: it decides
   * which screen a tap opens and whether the conversation is offered, and a
   * client that has to ask separately gets both wrong until it has asked. */
  threads?: number
  blocked?: number
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

  'agent.connect': (request: RequestId, session: string, agent: string, command: string[]) =>
    call(request, 'agent.connect', { session, agent, command }),

  'agent.threads': (request: RequestId, session: string) =>
    call(request, 'agent.threads', { session }),

  'agent.interrupt': (request: RequestId, session: string) =>
    call(request, 'agent.interrupt', { session }),

  'fs.list': (request: RequestId, workspace: string, path = '') =>
    call(request, 'fs.list', { workspace, path }),

  'git.status': (request: RequestId, workspace: string) =>
    call(request, 'git.status', { workspace }),

  'fanout.start': (
    request: RequestId,
    workspace: string,
    name: string,
    prompt: string,
    agents: string[],
  ) => call(request, 'fanout.start', { workspace, name, prompt, agents }),

  'fanout.status': (request: RequestId, name?: string) =>
    call(request, 'fanout.status', name === undefined ? {} : { name }),

  'fanout.choose': (request: RequestId, name: string, agent: string) =>
    call(request, 'fanout.choose', { name, agent }),

  'worktree.list': (request: RequestId, workspace: string) =>
    call(request, 'worktree.list', { workspace }),

  'worktree.add': (request: RequestId, workspace: string, path: string, branch: string) =>
    call(request, 'worktree.add', { workspace, path, branch }),

  'worktree.remove': (request: RequestId, workspace: string, path: string, force = false) =>
    call(request, 'worktree.remove', { workspace, path, force }),

  'git.hunks': (request: RequestId, workspace: string, path: string, staged = false) =>
    call(request, 'git.hunks', { workspace, path, staged }),

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

  'layout.open': (
    request: RequestId,
    workspace: string,
    panes: { program?: string; cwd?: string; title?: string }[],
  ) => call(request, 'layout.open', { workspace, panes }),

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

  'voice.providers': (request: RequestId) => call(request, 'voice.providers', {}),

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

  'session.restart': (request: RequestId, session: string, program?: string) =>
    call(request, 'session.restart', {
      session,
      ...(program === undefined ? {} : { program }),
    }),

  'session.blocks': (request: RequestId, session: string, failedOnly = false) =>
    call(request, 'session.blocks', { session, failed_only: failedOnly }),

  'session.snapshot': (request: RequestId, session: string) =>
    call(request, 'session.snapshot', { session }),

  'shell.integration': (request: RequestId, session?: string) =>
    call(request, 'shell.integration', session === undefined ? {} : { session }),

  'usage.report': (request: RequestId, session?: string) =>
    call(request, 'usage.report', session === undefined ? {} : { session }),

  'config.schema': (request: RequestId) => call(request, 'config.schema', {}),

  'config.get': (request: RequestId, key?: string) =>
    call(request, 'config.get', key === undefined ? {} : { key }),

  'keys.cheatsheet': (request: RequestId) => call(request, 'keys.cheatsheet', {}),

  // The structured surfaces. A phone is about forty columns at a readable size,
  // so the grid is the fallback and these are what it is actually read through.
  'session.scrollback': (request: RequestId, session: string, from = 0, lines = 200) =>
    call(request, 'session.scrollback', { session, from, lines }),

  'block.body': (request: RequestId, session: string, block: string, from = 0, lines = 200) =>
    call(request, 'block.body', { session, block, from, lines }),

  'search.session': (
    request: RequestId,
    session: string,
    query: string,
    caseSensitive = false,
    limit = 100,
  ) =>
    call(request, 'search.session', {
      session,
      query,
      case_sensitive: caseSensitive,
      limit,
    }),

  'session.transcript': (request: RequestId, session: string, from = 0, limit = 500) =>
    call(request, 'session.transcript', { session, from, limit }),

  'agent.offers': (request: RequestId, session: string) =>
    call(request, 'agent.offers', { session }),

  'identity.list': (request: RequestId) => call(request, 'identity.list', {}),

  'identity.revoke': (request: RequestId, identity: string) =>
    call(request, 'identity.revoke', { identity }),

  'presence.list': (request: RequestId) => call(request, 'presence.list', {}),

  'audit.list': (request: RequestId, limit = 200) =>
    call(request, 'audit.list', { limit }),

  'instance.metrics': (request: RequestId) => call(request, 'instance.metrics', {}),

  'doctor.report': (request: RequestId, whatHappened = '') =>
    call(request, 'doctor.report', { what_happened: whatHappened }),

  'doctor.run': (request: RequestId, group?: string) =>
    call(request, 'doctor.run', group === undefined ? {} : { group }),

  'invite.create': (request: RequestId) => call(request, 'invite.create', {}),

  'device.list': (request: RequestId) => call(request, 'device.list', {}),

  'device.revoke': (request: RequestId, device: string) =>
    call(request, 'device.revoke', { device }),

  'media.describe': (request: RequestId, data: string) =>
    call(request, 'media.describe', { data }),

  'config.import': (request: RequestId, source: string, settings: unknown) =>
    call(request, 'config.import', { source, settings }),

  'block.rerun': (request: RequestId, session: string, block: string, epoch: number) =>
    call(request, 'block.rerun', { session, block, epoch }),

  'queue.enqueue': (request: RequestId, session: string, text: string) =>
    call(request, 'queue.enqueue', { session, text }),

  'queue.remove': (request: RequestId, session: string, index: number) =>
    call(request, 'queue.remove', { session, index }),

  'onboarding.hint_state': (request: RequestId) =>
    call(request, 'onboarding.hint_state', {}),

  // `hint` omitted turns the lot off. Deliberately one call rather than two:
  // dismissing one and dismissing all are the same decision at two scopes, and
  // splitting them invites a client that offers only the nuclear option.
  'onboarding.dismiss_hints': (request: RequestId, hint?: string) =>
    call(request, 'onboarding.dismiss_hints', hint === undefined ? {} : { hint }),

  'media.put': (request: RequestId, content: string, origin: string) =>
    call(request, 'media.put', { content, origin }),

  'media.get': (request: RequestId, digest: string) =>
    call(request, 'media.get', { digest }),

  'device.rename': (request: RequestId, device: string, label: string) =>
    call(request, 'device.rename', { device, label }),

  'device.revoke_all': (request: RequestId, keep: string) =>
    call(request, 'device.revoke_all', { keep }),

  'store.retention': (request: RequestId, query: Record<string, unknown>) =>
    call(request, 'store.retention', query),

  'identity.reauth_check': (request: RequestId, check: Record<string, unknown>) =>
    call(request, 'identity.reauth_check', check),

  'keys.check_file': (
    request: RequestId,
    leader: string,
    bindings: [string, string][],
    keymap?: string,
  ) =>
    call(request, 'keys.check_file', {
      leader,
      bindings,
      ...(keymap === undefined ? {} : { keymap }),
    }),

  'launch.preview': (request: RequestId, launch: Record<string, unknown>) =>
    call(request, 'launch.preview', { launch }),

  'config.reload_class': (request: RequestId, keys: string[]) =>
    call(request, 'config.reload_class', { keys }),

  'security.publish_check': (request: RequestId, config: Record<string, unknown> = {}) =>
    call(request, 'security.publish_check', config),

  'security.bind_check': (
    request: RequestId,
    binds: string[],
    options: {
      auth_backend?: boolean
      tailnet_identity?: boolean
      tls?: boolean
      trust_proxy?: boolean
      default_role?: string
    } = {},
  ) => call(request, 'security.bind_check', { binds, ...options }),

  'recall.policy': (request: RequestId, indexReasoning = false) =>
    call(request, 'recall.policy', { index_reasoning: indexReasoning }),

  'session.timeline': (
    request: RequestId,
    events: { seq: number; at: number; kind: string; label: string }[],
  ) => call(request, 'session.timeline', { events }),

  'agent.compare': (request: RequestId, runs: Record<string, unknown>[]) =>
    call(request, 'agent.compare', { runs }),

  'agent.attention': (
    request: RequestId,
    observation: {
      working_secs?: number
      baseline_working_secs?: number
      silent_secs?: number
      baseline_gap_secs?: number
      recent_calls?: string[]
      consecutive_failures?: number
      last_failure?: string | null
      tokens_since_file_change?: number
      since_file_change_secs?: number
    } = {},
  ) => call(request, 'agent.attention', observation),

  'search.explain': (
    request: RequestId,
    candidates: {
      id: string
      kind: string
      bm25: number
      age_secs: number
      in_scope?: boolean
      exit?: number | null
      session_active?: boolean
    }[],
  ) => call(request, 'search.explain', { candidates }),

  'layout.save': (request: RequestId, name: string, panes: string[], preset: string) =>
    call(request, 'layout.save', { name, panes, preset }),

  'layout.restore': (
    request: RequestId,
    name: string,
    alive: string[],
    width: number,
    height: number,
  ) => call(request, 'layout.restore', { name, alive, width, height }),

  'pane.navigate': (
    request: RequestId,
    panes: {
      id: string
      left: number
      top: number
      right: number
      bottom: number
      hidden?: boolean
      placement_index: number
    }[],
    from: string,
    direction: string,
    wrap = false,
  ) => call(request, 'pane.navigate', { panes, from, direction, wrap }),

  'keys.list': (request: RequestId) => call(request, 'keys.list', {}),

  'keys.explain': (request: RequestId, chord: string) =>
    call(request, 'keys.explain', { chord }),

  'keys.registry': (request: RequestId) => call(request, 'keys.registry', {}),

  'keys.flavour': (request: RequestId, flavour: string) =>
    call(request, 'keys.flavour', { flavour }),

  'keys.audit': (request: RequestId) => call(request, 'keys.audit', {}),

  'keys.conflicts': (request: RequestId, chord: string) =>
    call(request, 'keys.conflicts', { chord }),

  'open.remote_list': (request: RequestId) => call(request, 'open.remote_list', {}),

  'open.remote_discard': (request: RequestId, path: string) =>
    call(request, 'open.remote_discard', { path }),

  'open.rules_list': (request: RequestId) => call(request, 'open.rules_list', {}),

  'open.rules_test': (request: RequestId, text: string) =>
    call(request, 'open.rules_test', { text }),

  'open.click': (
    request: RequestId,
    modifiers: {
      mouse_reporting?: boolean
      shift?: boolean
      super_key?: boolean
      alt?: boolean
    } = {},
  ) => call(request, 'open.click', modifiers),

  'open.resolve': (request: RequestId, path: string, roots: string[], line?: number) =>
    call(request, 'open.resolve', {
      path,
      roots,
      ...(line === undefined ? {} : { line }),
    }),

  'ssh.plan': (
    request: RequestId,
    target: string,
    remoteHasOmt = false,
    bootstrap?: string,
  ) =>
    call(request, 'ssh.plan', {
      target,
      remote_has_omt: remoteHasOmt,
      ...(bootstrap === undefined ? {} : { bootstrap }),
    }),

  'media.paste_tier': (
    request: RequestId,
    probe: {
      omt_on_both_ends?: boolean
      reverse_socket_answered?: boolean
      osc_companion_answered?: boolean
      terminal_upload_confirmed?: boolean
    } = {},
  ) => call(request, 'media.paste_tier', probe),

  'media.attach': (request: RequestId, agent: string, path: string) =>
    call(request, 'media.attach', { agent, path }),

  'media.clipboard': (request: RequestId, text: string, term?: string) =>
    call(request, 'media.clipboard', term === undefined ? { text } : { text, term }),

  'media.list': (request: RequestId) => call(request, 'media.list', {}),

  'onboarding.disclosures': (request: RequestId, trigger?: string) =>
    call(request, 'onboarding.disclosures', trigger === undefined ? {} : { trigger }),

  'help.topics': (request: RequestId) => call(request, 'help.topics', {}),

  'help.render': (request: RequestId, topic: string) =>
    call(request, 'help.render', { topic }),

  'setup.detect_agents': (request: RequestId) => call(request, 'setup.detect_agents', {}),

  'term.profile': (request: RequestId) => call(request, 'term.profile', {}),

  // `migrate.tmux_preview`, not `migrate.tmux.preview` as the doc spells it: a
  // capability name is its group and its verb, two levels and no more.
  'migrate.tmux_preview': (request: RequestId, config: string) =>
    call(request, 'migrate.tmux_preview', { config }),

  'uninstall.plan': (request: RequestId, keepConfig = false, keepData = false) =>
    call(request, 'uninstall.plan', { keep_config: keepConfig, keep_data: keepData }),

  // `plan` is the hash the preview returned, and apply re-checks it against the
  // disk. A confirm button that could not name which plan it confirmed would be
  // confirming whatever the machine looks like at the moment it is pressed.
  'uninstall.apply': (
    request: RequestId,
    plan: string,
    accept: boolean,
    keepConfig = false,
    keepData = false,
  ) =>
    call(request, 'uninstall.apply', {
      plan,
      accept,
      keep_config: keepConfig,
      keep_data: keepData,
    }),
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

/**
 * Typed calls for each capability this client handles.
 *
 * The list in `generated/handlers.json` is what the parity gate checks, and it
 * must mean "the web client implements this" rather than "somebody remembered
 * to add a line". So every name there has a function here, and a test asserts
 * the two agree — otherwise the gate degrades into a list that passes itself.
 */
import { CAPABILITY_INFO } from './generated/catalog.js';
/** Build the call for a capability. */
function call(request, capability, input) {
    // A command carries an intent id and a query must not. Minted here rather
    // than by each caller, because the rule is "every command, always" and a
    // rule enforced at seventeen call sites is a rule with a hole in it.
    return isCommand(capability)
        ? { t: 'call', request, capability, input, intent: mintIntent() }
        : { t: 'call', request, capability, input };
}
/** Whether a capability mutates, according to the catalog the server generated. */
export function isCommand(capability) {
    return CAPABILITY_INFO[capability]?.kind === 'command';
}
/**
 * A fresh intent id.
 *
 * Minted before the message goes out — at intent time, not on arrival —
 * because the whole point is that a client which lost its connection can
 * repeat the identical call and be recognised rather than acted on twice.
 */
function mintIntent() {
    return crypto.randomUUID();
}
/** Every capability this client can invoke, by name. */
export const HANDLERS = {
    'instance.info': (request) => call(request, 'instance.info', {}),
    'instance.catalog': (request) => call(request, 'instance.catalog', {}),
    'events.subscribe': (request, since = {}) => call(request, 'events.subscribe', { since }),
    'workspace.list': (request) => call(request, 'workspace.list', {}),
    'workspace.open': (request, root) => call(request, 'workspace.open', { root }),
    'session.list': (request, workspace) => call(request, 'session.list', workspace === undefined ? {} : { workspace }),
    'session.close': (request, session) => call(request, 'session.close', { session }),
    'agent.connect': (request, session, agent, command) => call(request, 'agent.connect', { session, agent, command }),
    'agent.threads': (request, session) => call(request, 'agent.threads', { session }),
    'agent.interrupt': (request, session) => call(request, 'agent.interrupt', { session }),
    'fs.list': (request, workspace, path = '') => call(request, 'fs.list', { workspace, path }),
    'git.status': (request, workspace) => call(request, 'git.status', { workspace }),
    'fanout.start': (request, workspace, name, prompt, agents) => call(request, 'fanout.start', { workspace, name, prompt, agents }),
    'fanout.status': (request, name) => call(request, 'fanout.status', name === undefined ? {} : { name }),
    'fanout.choose': (request, name, agent) => call(request, 'fanout.choose', { name, agent }),
    'worktree.list': (request, workspace) => call(request, 'worktree.list', { workspace }),
    'worktree.add': (request, workspace, path, branch) => call(request, 'worktree.add', { workspace, path, branch }),
    'worktree.remove': (request, workspace, path, force = false) => call(request, 'worktree.remove', { workspace, path, force }),
    'git.hunks': (request, workspace, path, staged = false) => call(request, 'git.hunks', { workspace, path, staged }),
    'git.diff': (request, workspace, staged = false) => call(request, 'git.diff', { workspace, staged }),
    // The epoch is required, not optional: input already in flight when the
    // writer token changed hands must be rejected rather than landing in
    // somebody else's command line.
    'session.write': (request, session, text, epoch) => call(request, 'session.write', { session, text, epoch }),
    'session.resize': (request, session, cols, rows) => call(request, 'session.resize', { session, cols, rows }),
    'session.read': (request, session, history = 0) => call(request, 'session.read', { session, history }),
    'session.create': (request, workspace, program, cols = 80, rows = 24) => call(request, 'session.create', {
        workspace,
        ...(program === undefined ? {} : { program }),
        cols,
        rows,
    }),
    'layout.open': (request, workspace, panes) => call(request, 'layout.open', { workspace, panes }),
    'pane.list': (request, workspace) => call(request, 'pane.list', { workspace }),
    'pane.open': (request, workspace, session) => call(request, 'pane.open', { workspace, session }),
    'pane.close': (request, workspace, pane) => call(request, 'pane.close', { workspace, pane }),
    'pane.focus': (request, workspace, pane) => call(request, 'pane.focus', { workspace, pane }),
    'state.save': (request, path) => call(request, 'state.save', path === undefined ? {} : { path }),
    'state.restore': (request, path) => call(request, 'state.restore', path === undefined ? {} : { path }),
    'recall.suggest': (request, prefix, workspace, limit = 10) => call(request, 'recall.suggest', { prefix, workspace, limit }),
    'recall.record': (request, command, workspace, session, exitCode) => call(request, 'recall.record', {
        command,
        workspace,
        session,
        ...(exitCode === undefined ? {} : { exit_code: exitCode }),
    }),
    'plugin.install': (request, id, name, version, permissions, grant, entry = []) => call(request, 'plugin.install', { id, name, version, permissions, grant, entry }),
    'plugin.start': (request, id) => call(request, 'plugin.start', { id }),
    'plugin.list': (request) => call(request, 'plugin.list', {}),
    'plugin.enable': (request, id, enabled) => call(request, 'plugin.enable', { id, enabled }),
    'job.create': (request, name, workspace, run, everySeconds = 0) => call(request, 'job.create', { name, workspace, run, every_seconds: everySeconds }),
    'job.list': (request) => call(request, 'job.list', {}),
    'voice.providers': (request) => call(request, 'voice.providers', {}),
    'voice.append': (request, session, text, finalChunk = false) => call(request, 'voice.append', { session, text, final_chunk: finalChunk }),
    'voice.clear': (request, session) => call(request, 'voice.clear', { session }),
    'theme.get': (request) => call(request, 'theme.get', {}),
    'open.recognize': (request, line) => call(request, 'open.recognize', { line }),
    'fs.read': (request, workspace, path, chunk = 0) => call(request, 'fs.read', { workspace, path, chunk }),
    'fs.write': (request, workspace, path, data, chunk = 0, chunks = 1) => call(request, 'fs.write', { workspace, path, data, chunk, chunks }),
    'interaction.list': (request, session) => call(request, 'interaction.list', session === undefined ? {} : { session }),
    'interaction.resolve': (request, interaction, option, text) => call(request, 'interaction.resolve', {
        interaction,
        option,
        ...(text === undefined ? {} : { text }),
    }),
    'session.acquire': (request, session, force = false) => call(request, 'session.acquire', { session, force }),
    'session.release': (request, session) => call(request, 'session.release', { session }),
    'session.restart': (request, session, program) => call(request, 'session.restart', {
        session,
        ...(program === undefined ? {} : { program }),
    }),
    'session.blocks': (request, session, failedOnly = false) => call(request, 'session.blocks', { session, failed_only: failedOnly }),
    'session.snapshot': (request, session) => call(request, 'session.snapshot', { session }),
    'shell.integration': (request, session) => call(request, 'shell.integration', session === undefined ? {} : { session }),
    'usage.report': (request, session) => call(request, 'usage.report', session === undefined ? {} : { session }),
    'config.schema': (request) => call(request, 'config.schema', {}),
    'config.get': (request, key) => call(request, 'config.get', key === undefined ? {} : { key }),
    'keys.cheatsheet': (request) => call(request, 'keys.cheatsheet', {}),
    // The structured surfaces. A phone is about forty columns at a readable size,
    // so the grid is the fallback and these are what it is actually read through.
    'session.scrollback': (request, session, from = 0, lines = 200) => call(request, 'session.scrollback', { session, from, lines }),
    'block.body': (request, session, block, from = 0, lines = 200) => call(request, 'block.body', { session, block, from, lines }),
    'search.session': (request, session, query, caseSensitive = false, limit = 100) => call(request, 'search.session', {
        session,
        query,
        case_sensitive: caseSensitive,
        limit,
    }),
    'session.transcript': (request, session, from = 0, limit = 500) => call(request, 'session.transcript', { session, from, limit }),
    'agent.offers': (request, session) => call(request, 'agent.offers', { session }),
    'identity.list': (request) => call(request, 'identity.list', {}),
    'identity.revoke': (request, identity) => call(request, 'identity.revoke', { identity }),
    'presence.list': (request) => call(request, 'presence.list', {}),
    'audit.list': (request, limit = 200) => call(request, 'audit.list', { limit }),
    'instance.metrics': (request) => call(request, 'instance.metrics', {}),
    'doctor.report': (request, whatHappened = '') => call(request, 'doctor.report', { what_happened: whatHappened }),
    'doctor.run': (request, group) => call(request, 'doctor.run', group === undefined ? {} : { group }),
    'invite.create': (request) => call(request, 'invite.create', {}),
    'device.list': (request) => call(request, 'device.list', {}),
    'device.revoke': (request, device) => call(request, 'device.revoke', { device }),
    'media.describe': (request, data) => call(request, 'media.describe', { data }),
    'config.import': (request, source, settings) => call(request, 'config.import', { source, settings }),
    'block.rerun': (request, session, block, epoch) => call(request, 'block.rerun', { session, block, epoch }),
    // A binding, never a session. A replayed enqueue against a session whose
    // agent has since exited types the text into a shell prompt, where it runs
    // as a command.
    // The primitive that stops every script being a sleep loop. `blocked` is a
    // state and matches at once; `interaction` is an edge and matches only a card
    // that opens after the wait began.
    'agent.wait': (request, session, until, timeoutMs, immediate) => call(request, 'agent.wait', { session, until, timeout_ms: timeoutMs, immediate }),
    'session.capture': (request, session, format, lines) => call(request, 'session.capture', { session, format, lines }),
    // 22 §10.1's job registry. Named `task` because `job` already means a
    // scheduled job in the catalog, and one name for two things would resolve by
    // load order.
    'task.list': (request, includeFinished) => call(request, 'task.list', { include_finished: includeFinished }),
    'task.get': (request, task) => call(request, 'task.get', { task }),
    // Cooperative: it reports `cancelling`, not `cancelled`.
    'task.cancel': (request, task) => call(request, 'task.cancel', { task }),
    'upgrade.check': (request, channel) => call(request, 'upgrade.check', { channel }),
    'instance.shutdown': (request, force) => call(request, 'instance.shutdown', { force }),
    'remote.probe': (request, host) => call(request, 'remote.probe', { host }),
    // 20 §12.2's instance-scoped query. `session.history` and
    // `workspace.history` would be pre-scoped conveniences over this one.
    'history.query': (request, scope, of, filter, mode) => call(request, 'history.query', { scope, of, filter, mode }),
    'history.forget': (request, filter, scope, of) => call(request, 'history.forget', { filter, scope, of }),
    'queue.enqueue': (request, binding, state, text) => call(request, 'queue.enqueue', { binding, state, text }),
    'queue.list': (request, binding) => call(request, 'queue.list', { binding }),
    // Named by text as well as by id: an id alone is a position, and a position
    // in a draining queue is not a stable target.
    'queue.remove': (request, binding, state, entry, expectedText) => call(request, 'queue.remove', { binding, state, entry, expected_text: expectedText }),
    // Both are unsupported on an agent's own queue, and the client hides the
    // controls rather than disabling them.
    'queue.edit': (request, binding, state, entry, expectedVersion, text) => call(request, 'queue.edit', {
        binding,
        state,
        entry,
        expected_version: expectedVersion,
        text,
    }),
    'queue.reorder': (request, binding, state, order, expectedVersion) => call(request, 'queue.reorder', { binding, state, order, expected_version: expectedVersion }),
    'onboarding.hint_state': (request) => call(request, 'onboarding.hint_state', {}),
    // `hint` omitted turns the lot off. Deliberately one call rather than two:
    // dismissing one and dismissing all are the same decision at two scopes, and
    // splitting them invites a client that offers only the nuclear option.
    'onboarding.dismiss_hints': (request, hint) => call(request, 'onboarding.dismiss_hints', hint === undefined ? {} : { hint }),
    'media.put': (request, content, origin) => call(request, 'media.put', { content, origin }),
    // Rules for reading an agent's screen. Reachable from here because 03's
    // parity contract says a capability the instance offers is one every client
    // can call — and because somebody whose agent changed its wording is as
    // likely to be holding a phone as a keyboard.
    'agent_rules.capture': (request, session) => call(request, 'agent_rules.capture', { session }),
    'agent_rules.inspect': (request, rules, agent) => call(request, 'agent_rules.inspect', { rules, agent }),
    'agent_rules.import': (request, rules, agent) => call(request, 'agent_rules.import', { rules, agent }),
    'agent_rules.explain': (request, session) => call(request, 'agent_rules.explain', { session }),
    // A roster of six things called `shell` is what omt shows without this, and
    // a phone is where somebody most needs to tell them apart.
    'session.rename': (request, session, title) => call(request, 'session.rename', { session, title }),
    // Without these an agent's structured question never reaches omt, so a card
    // that could have been answered on a phone is a prompt only the terminal can
    // see. Reachable from here because the phone is where somebody notices that
    // the cards never arrive.
    'agent.hooks_plan': (request, agent) => call(request, 'agent.hooks_plan', { agent }),
    'agent.hooks_install': (request, agent) => call(request, 'agent.hooks_install', { agent }),
    // What would be run to continue an agent's conversation rather than start a
    // new one. Reachable from a phone because that is where somebody is when
    // they find out a session died — and the answer is a command they will type
    // somewhere else, so it is worth being able to read it from here.
    'agent.resume_plan': (request, session) => call(request, 'agent.resume_plan', { session }),
    'media.materialize': (request, digest, name) => call(request, 'media.materialize', {
        digest,
        ...(name === undefined ? {} : { name }),
    }),
    'media.get': (request, digest) => call(request, 'media.get', { digest }),
    'device.rename': (request, device, label) => call(request, 'device.rename', { device, label }),
    'device.revoke_all': (request, keep) => call(request, 'device.revoke_all', { keep }),
    'store.paths': (request) => call(request, 'store.paths', {}),
    'store.migrate_status': (request, marker) => call(request, 'store.migrate_status', { marker }),
    'store.redaction_test': (request, text) => call(request, 'store.redaction_test', { text }),
    'store.retention': (request, query) => call(request, 'store.retention', query),
    'identity.reauth_check': (request, check) => call(request, 'identity.reauth_check', check),
    'keys.check_file': (request, leader, bindings, keymap) => call(request, 'keys.check_file', {
        leader,
        bindings,
        ...(keymap === undefined ? {} : { keymap }),
    }),
    'launch.preview': (request, launch) => call(request, 'launch.preview', { launch }),
    'config.reload_class': (request, keys) => call(request, 'config.reload_class', { keys }),
    'security.publish_check': (request, config = {}) => call(request, 'security.publish_check', config),
    'security.bind_check': (request, binds, options = {}) => call(request, 'security.bind_check', { binds, ...options }),
    'recall.policy': (request, indexReasoning = false) => call(request, 'recall.policy', { index_reasoning: indexReasoning }),
    'session.timeline': (request, events) => call(request, 'session.timeline', { events }),
    'agent.compare': (request, runs) => call(request, 'agent.compare', { runs }),
    'agent.attention': (request, observation = {}) => call(request, 'agent.attention', observation),
    'search.explain': (request, candidates) => call(request, 'search.explain', { candidates }),
    'layout.save': (request, name, panes, preset) => call(request, 'layout.save', { name, panes, preset }),
    'layout.restore': (request, name, alive, width, height) => call(request, 'layout.restore', { name, alive, width, height }),
    'pane.navigate': (request, panes, from, direction, wrap = false) => call(request, 'pane.navigate', { panes, from, direction, wrap }),
    'keys.list': (request) => call(request, 'keys.list', {}),
    'keys.explain': (request, chord) => call(request, 'keys.explain', { chord }),
    'keys.registry': (request) => call(request, 'keys.registry', {}),
    'keys.flavour': (request, flavour) => call(request, 'keys.flavour', { flavour }),
    'keys.audit': (request) => call(request, 'keys.audit', {}),
    'keys.conflicts': (request, chord) => call(request, 'keys.conflicts', { chord }),
    'open.remote_list': (request) => call(request, 'open.remote_list', {}),
    'open.remote_discard': (request, path) => call(request, 'open.remote_discard', { path }),
    'open.rules_list': (request) => call(request, 'open.rules_list', {}),
    'open.rules_test': (request, text) => call(request, 'open.rules_test', { text }),
    'open.click': (request, modifiers = {}) => call(request, 'open.click', modifiers),
    'open.resolve': (request, path, roots, line) => call(request, 'open.resolve', {
        path,
        roots,
        ...(line === undefined ? {} : { line }),
    }),
    'ssh.plan': (request, target, remoteHasOmt = false, bootstrap) => call(request, 'ssh.plan', {
        target,
        remote_has_omt: remoteHasOmt,
        ...(bootstrap === undefined ? {} : { bootstrap }),
    }),
    'media.paste_tier': (request, probe = {}) => call(request, 'media.paste_tier', probe),
    'media.attach': (request, agent, path) => call(request, 'media.attach', { agent, path }),
    'media.clipboard': (request, text, term) => call(request, 'media.clipboard', term === undefined ? { text } : { text, term }),
    'media.list': (request) => call(request, 'media.list', {}),
    'onboarding.disclosures': (request, trigger) => call(request, 'onboarding.disclosures', trigger === undefined ? {} : { trigger }),
    'help.topics': (request) => call(request, 'help.topics', {}),
    'help.render': (request, topic) => call(request, 'help.render', { topic }),
    'setup.detect_agents': (request) => call(request, 'setup.detect_agents', {}),
    'term.profile': (request) => call(request, 'term.profile', {}),
    // `migrate.tmux_preview`, not `migrate.tmux.preview` as the doc spells it: a
    // capability name is its group and its verb, two levels and no more.
    'migrate.tmux_preview': (request, config) => call(request, 'migrate.tmux_preview', { config }),
    'uninstall.plan': (request, keepConfig = false, keepData = false) => call(request, 'uninstall.plan', { keep_config: keepConfig, keep_data: keepData }),
    // `plan` is the hash the preview returned, and apply re-checks it against the
    // disk. A confirm button that could not name which plan it confirmed would be
    // confirming whatever the machine looks like at the moment it is pressed.
    'uninstall.apply': (request, plan, accept, keepConfig = false, keepData = false) => call(request, 'uninstall.apply', {
        plan,
        accept,
        keep_config: keepConfig,
        keep_data: keepData,
    }),
};
/** Whether this client can invoke a capability. */
export function handles(capability) {
    return capability in HANDLERS;
}
/** Every capability this client handles, sorted. */
export function handledCapabilities() {
    return Object.keys(HANDLERS).sort();
}

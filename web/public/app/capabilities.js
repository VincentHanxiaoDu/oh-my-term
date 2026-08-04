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
    'agent.threads': (request, session) => call(request, 'agent.threads', { session }),
    'agent.interrupt': (request, session) => call(request, 'agent.interrupt', { session }),
    'fs.list': (request, workspace, path = '') => call(request, 'fs.list', { workspace, path }),
    'git.status': (request, workspace) => call(request, 'git.status', { workspace }),
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
    'session.acquire': (request, session, force = false) => call(request, 'session.acquire', { session, force }),
    'session.release': (request, session) => call(request, 'session.release', { session }),
    'session.snapshot': (request, session) => call(request, 'session.snapshot', { session }),
    'config.get': (request, key) => call(request, 'config.get', key === undefined ? {} : { key }),
    'keys.cheatsheet': (request) => call(request, 'keys.cheatsheet', {}),
};
/** Whether this client can invoke a capability. */
export function handles(capability) {
    return capability in HANDLERS;
}
/** Every capability this client handles, sorted. */
export function handledCapabilities() {
    return Object.keys(HANDLERS).sort();
}

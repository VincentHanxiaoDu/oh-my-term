/**
 * The client's whole view of an instance, in one place.
 *
 * Everything a surface renders comes from here, and everything a surface does
 * goes through `call`. That is not architecture for its own sake: it means the
 * rules that matter — a position advances only for delivered events, an
 * answering affordance comes from `deliverable` — are enforced once rather
 * than in every component that happens to render a card.
 */
import { HANDLERS, isCommand, } from './capabilities.js';
import { isAnswerable, isTerminal } from './protocol.js';
import { InstanceClient } from './session.js';
import { grid, summarize } from './threads.js';
/**
 * Whether a failure means "not here" rather than "went wrong".
 *
 * `unsupported` is what the instance answers when a thing cannot be done in
 * this place at all — a workspace with no git history, a capability an older
 * instance does not have. Rendering those as errors puts an alarm on a screen
 * whose honest content is one calm sentence, and an alarm somebody learns to
 * ignore is worse than none.
 */
export function isUnsupported(failure) {
    return (typeof failure === 'object' &&
        failure !== null &&
        failure.code === 'unsupported');
}
/**
 * The client's state, and the only way to change it.
 */
/**
 * How long a burst of events is allowed to accumulate before one refresh.
 *
 * Short enough to feel immediate, long enough that a hundred events from a
 * running build produce one call rather than a hundred.
 */
export const REFRESH_COALESCE_MS = 80;
export class Store {
    #client;
    #sink;
    #workspaces = [];
    #sessions = [];
    #threads = new Map();
    #problems = [];
    #pending = new Map();
    #listeners = new Set();
    /** A refresh already scheduled, so a burst of events costs one call. */
    #pendingRefresh = null;
    /** Commands minted while the connection was gone. */
    #queued = [];
    constructor(device, sink) {
        this.#client = new InstanceClient(device);
        this.#sink = sink;
    }
    /** What a surface renders right now. */
    get state() {
        const connection = this.#client.connection;
        const open = this.#client.openInteractions();
        return {
            connection: connection.state,
            ...(connection.state === 'refused' ? { refusal: connection.detail } : {}),
            workspaces: this.#workspaces,
            sessions: this.#sessions,
            threads: Object.fromEntries(this.#threads),
            open,
            needsYou: open.filter((i) => isAnswerable(i.deliverable)).length,
            problems: this.#problems,
        };
    }
    /** Be told when anything changes. */
    subscribe(listener) {
        this.#listeners.add(listener);
        return () => this.#listeners.delete(listener);
    }
    /** Open the connection. */
    connect(token) {
        for (const message of this.#client.hello(token)) {
            this.#sink.send(message);
        }
        this.#emit();
    }
    /**
     * Invoke a capability.
     *
     * Rejects if this instance does not offer it, *before* sending — a call that
     * went out and came back "unknown capability" costs a round trip to learn
     * something the welcome already said.
     */
    call(capability, ...args) {
        if (!this.#client.can(capability)) {
            return Promise.reject({
                capability,
                code: 'unsupported',
                message: `this instance does not offer ${capability}`,
            });
        }
        const request = this.#client.request();
        const build = HANDLERS[capability];
        const message = build(request, ...args);
        return new Promise((resolve, reject) => {
            this.#pending.set(keyOf(request), { capability, resolve, reject });
            if (this.#client.connection.state !== 'connected' && isCommand(capability)) {
                // Held rather than failed. A phone loses its network in a lift and in a
                // tunnel, and a tap that vanishes because of five seconds of no signal
                // is a tap somebody makes again — which for a command means doing it
                // twice.
                //
                // Only commands: a query held for later answers a question nobody is
                // still asking. And only commands carry an intent id, which is what
                // makes the instance recognise the replay as the same intent rather
                // than a second one.
                this.#queued.push(message);
                return;
            }
            this.#sink.send(message);
        });
    }
    /** What is waiting for the connection to come back. */
    get queued() {
        return this.#queued.length;
    }
    /**
     * Send everything that was held while the connection was gone.
     *
     * Each carries the intent id it was minted with, so an instance that already
     * saw one recognises the repeat and does not run it twice — which is the
     * whole reason intent ids are minted at intent time rather than at send time.
     */
    #flush() {
        const waiting = this.#queued;
        this.#queued = [];
        for (const message of waiting) {
            this.#sink.send(message);
        }
    }
    /**
     * Re-read state shortly, coalescing a burst into one call.
     *
     * A busy session emits events faster than a round trip, and refreshing per
     * event would queue more calls than the socket can answer — the client would
     * fall further behind the more there was to see.
     */
    #refreshSoon() {
        if (this.#pendingRefresh !== null) {
            return;
        }
        this.#pendingRefresh = setTimeout(() => {
            this.#pendingRefresh = null;
            void this.refresh();
        }, REFRESH_COALESCE_MS);
    }
    /** Apply a message from the server. */
    receive(message) {
        const outcome = this.#client.receive(message);
        if (message.t === 'result') {
            this.#settle(message);
        }
        if (message.t === 'welcome') {
            // The welcome says what this instance offers, not what it holds. Without
            // this the client sits on an empty roster forever, looking exactly like
            // an instance with nothing running.
            void this.refresh();
            // And subscribe, so what happens next *arrives* rather than being
            // discovered by asking again. Without it every screen polls: the roster
            // on a timer, the terminal six times a second, a phone with a warm
            // battery and a radio that never sleeps.
            this.#sink.send({ t: 'subscribe', since: {} });
            // Anything typed while the connection was gone goes now, in the order it
            // was made.
            this.#flush();
        }
        if (message.t === 'event') {
            // Something changed. What changed is in the event, but the cheapest
            // correct thing is to re-read what the visible screens are built from —
            // a roster of six is one call, and being right matters more here than
            // being clever.
            this.#refreshSoon();
        }
        if (message.t === 'goodbye') {
            this.#problems.unshift({
                message: message.detail,
                // A refused credential will not fix itself; anything else might.
                transient: message.reason !== 'unauthorized',
            });
        }
        if (outcome === 'gap' || outcome === 'resync-needed') {
            // Said out loud rather than papered over: a client that quietly
            // continued would be showing stale state as if it were current.
            this.#problems.unshift({
                message: 'missed some updates — refreshing',
                transient: true,
            });
            void this.refresh();
        }
        this.#emit();
    }
    /**
     * Re-read everything from the instance.
     *
     * Both calls go out together. Chaining them costs two round trips before the
     * roster can draw anything, and neither answer depends on the other — over a
     * phone link that is the difference between opening to a list and opening to
     * a blank screen that fills in twice.
     */
    async refresh() {
        try {
            const [workspaces, sessions] = (await Promise.all([
                this.call('workspace.list'),
                this.call('session.list'),
            ]));
            this.#workspaces = workspaces.workspaces;
            this.#sessions = sessions.sessions;
        }
        catch (e) {
            const failure = e;
            this.#problems.unshift({ message: failure.message, transient: true });
        }
        this.#emit();
    }
    /** Read one session's threads. */
    async loadThreads(session) {
        try {
            const out = (await this.call('agent.threads', session));
            this.#threads.set(session, out.threads);
        }
        catch (e) {
            this.#problems.unshift({ message: e.message, transient: true });
        }
        this.#emit();
    }
    /** The subagent grid for a session, ordered for rendering. */
    gridFor(session) {
        return grid(toThreads(this.#threads.get(session) ?? []));
    }
    /** The header line above that grid. */
    summaryFor(session) {
        return summarize(toThreads(this.#threads.get(session) ?? []));
    }
    /** Cards this client may actually answer. */
    answerable() {
        return this.#client.answerable();
    }
    /** Forget a problem the user has seen. */
    dismiss(index) {
        this.#problems.splice(index, 1);
        this.#emit();
    }
    #settle(message) {
        const key = keyOf(message.request);
        const pending = this.#pending.get(key);
        if (!pending) {
            // A result for a request this client never made, or made twice. Acting on
            // it would apply somebody else's answer.
            return;
        }
        this.#pending.delete(key);
        if (message.status === 'ok') {
            pending.resolve(message.output);
        }
        else {
            pending.reject({
                capability: pending.capability,
                code: message.error.code,
                message: message.error.message,
            });
        }
    }
    #emit() {
        const state = this.state;
        for (const listener of this.#listeners) {
            listener(state);
        }
    }
}
/** Wire summaries into the shape the grid renders. */
function toThreads(summaries) {
    return summaries.map((t) => ({
        id: t.id,
        is_subagent: t.is_subagent,
        state: stateOf(t.state),
        ...(t.label === undefined ? {} : { label: t.label }),
        open_interactions: t.open_interactions,
    }));
}
function stateOf(name) {
    switch (name) {
        case 'blocked':
            return { state: 'blocked', reason: 'unspecified' };
        case 'working':
            return { state: 'working' };
        case 'idle':
            return { state: 'idle' };
        case 'exited':
            return { state: 'exited' };
        default:
            return { state: 'unknown' };
    }
}
function keyOf(request) {
    return `${request.device}:${request.n}`;
}
export { isAnswerable, isTerminal };

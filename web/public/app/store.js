/**
 * The client's whole view of an instance, in one place.
 *
 * Everything a surface renders comes from here, and everything a surface does
 * goes through `call`. That is not architecture for its own sake: it means the
 * rules that matter — a position advances only for delivered events, an
 * answering affordance comes from `deliverable` — are enforced once rather
 * than in every component that happens to render a card.
 */
import { HANDLERS } from './capabilities.js';
import { isAnswerable, isTerminal } from './protocol.js';
import { InstanceClient } from './session.js';
import { grid, summarize } from './threads.js';
/**
 * The client's state, and the only way to change it.
 */
export class Store {
    #client;
    #sink;
    #workspaces = [];
    #sessions = [];
    #threads = new Map();
    #problems = [];
    #pending = new Map();
    #listeners = new Set();
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
            this.#sink.send(message);
        });
    }
    /** Apply a message from the server. */
    receive(message) {
        const outcome = this.#client.receive(message);
        if (message.t === 'result') {
            this.#settle(message);
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
    /** Re-read everything from the instance. */
    async refresh() {
        try {
            const workspaces = (await this.call('workspace.list'));
            this.#workspaces = workspaces.workspaces;
            const sessions = (await this.call('session.list'));
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

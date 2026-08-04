/**
 * The client's view of an instance, and the reconnect that keeps it honest.
 */
import { ResumeState, } from './resume.js';
import { PROTO_VERSION, isAnswerable, isTerminal, } from './protocol.js';
/**
 * One connection's state, driven by messages rather than by a socket.
 *
 * Taking messages instead of owning a WebSocket is what makes every rule here
 * testable without a server: reconnection, gap detection and capability
 * intersection are exactly the paths that are hard to exercise for real.
 */
export class InstanceClient {
    device;
    #connection = { state: 'disconnected' };
    #resume = new ResumeState();
    #interactions = new Map();
    #nextRequest = 1;
    constructor(device) {
        this.device = device;
    }
    /** What the connection can do right now. */
    get connection() {
        return this.#connection;
    }
    /** Where this client is in each stream. */
    get resume() {
        return this.#resume;
    }
    /** The hello to send, with whatever positions are already held. */
    hello(token) {
        this.#connection = { state: 'connecting' };
        const messages = [
            token === undefined
                ? { t: 'hello', proto: PROTO_VERSION, client: 'web' }
                : { t: 'hello', proto: PROTO_VERSION, client: 'web', token },
        ];
        messages.push({ t: 'subscribe', since: this.#resume.since() });
        return messages;
    }
    /** Mint a request id. */
    request() {
        return { device: this.device, n: this.#nextRequest++ };
    }
    /**
     * Whether a capability is usable here.
     *
     * A phone attached to several instances on different versions renders what
     * each supports and greys out the rest, rather than erroring on the first
     * mismatch — so this is a question every surface asks before it draws a
     * button, not one it discovers by calling.
     */
    can(capability) {
        return (this.#connection.state === 'connected' &&
            this.#connection.capabilities.has(capability));
    }
    /** Apply a message from the server. */
    receive(message) {
        switch (message.t) {
            case 'welcome':
                this.#connection = {
                    state: 'connected',
                    capabilities: new Set(message.capabilities),
                    role: message.role,
                };
                return undefined;
            case 'goodbye':
                this.#connection = {
                    state: 'refused',
                    reason: message.reason,
                    detail: message.detail,
                };
                return undefined;
            case 'resync':
                // The server says the gap is unrecoverable. Dropping the position is
                // the only honest response: keeping it would have the next reconnect
                // ask to resume from a point that no longer exists.
                this.#resume.reset(message.scope_key);
                return 'resync-needed';
            case 'event':
                return this.#applyEvent(message.event);
            case 'result':
                return undefined;
        }
    }
    #applyEvent(event) {
        const verdict = this.#resume.inspect(event);
        if (verdict.action !== 'deliver') {
            return verdict.action;
        }
        // The position advances only after the event has been applied, so a handler
        // that throws leaves the client able to ask for it again.
        this.#applyPayload(event);
        this.#resume.accept(event);
        return 'deliver';
    }
    #applyPayload(event) {
        const payload = event.payload;
        if (payload?.interaction) {
            const i = payload.interaction;
            // One transition event carrying the whole interaction, rather than four
            // narrow ones: a client that missed an earlier transition still ends up
            // correct, which matters because missing one is the normal case on a
            // phone.
            this.#interactions.set(i.id, i);
        }
    }
    /** Interactions still waiting for somebody. */
    openInteractions() {
        return [...this.#interactions.values()]
            .filter((i) => !isTerminal(i.state))
            .sort((a, b) => a.opened_at.localeCompare(b.opened_at));
    }
    /** Interactions this client may actually answer. */
    answerable() {
        return this.openInteractions().filter((i) => isAnswerable(i.deliverable));
    }
    /**
     * Why a card cannot be answered here, if it cannot.
     *
     * Surfaced rather than hidden: a card the user can see but not act on needs
     * to say why and point at the terminal, or it reads as a broken button.
     */
    whyNotAnswerable(i) {
        const d = i.deliverable;
        if (d.kind === 'none') {
            return d.reason;
        }
        if (isTerminal(i.state)) {
            return 'it has already been answered';
        }
        return undefined;
    }
    /** Drop everything and start clean. */
    disconnect() {
        this.#connection = { state: 'disconnected' };
    }
}

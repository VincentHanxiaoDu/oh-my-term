/**
 * Tracking where a client is in each stream.
 *
 * The rule: a position advances only for events actually delivered, in order.
 * A client that recorded a position for an event it dropped would resume past
 * it and never know — which is the failure the whole resume mechanism exists to
 * prevent, arriving through its own bookkeeping.
 */
import { scopeKey } from './protocol.js';
/** One client's position in every stream it follows. */
export class ResumeState {
    #positions = new Map();
    /** Where this client is in a scope, if it has seen anything. */
    position(key) {
        return this.#positions.get(key);
    }
    /** The resume map to send on reconnect. */
    since() {
        return Object.fromEntries(this.#positions);
    }
    /**
     * Decide what to do with an event, without recording anything.
     *
     * Separate from {@link accept} so a caller can look before it leaps: an event
     * that fails to render must not have advanced the position.
     */
    inspect(event) {
        const key = scopeKey(event.scope);
        const current = this.#positions.get(key);
        if (current === undefined) {
            return { action: 'deliver' };
        }
        if (event.seq <= current) {
            // A duplicate after a reconnect. Rendering it again would show the same
            // output twice, which on a terminal is indistinguishable from the program
            // having printed it twice.
            return { action: 'skip', why: 'already-seen' };
        }
        if (event.seq > current + 1) {
            // Something was lost. Continuing quietly leaves the client believing it
            // is current when it is not.
            return { action: 'gap', expected: current + 1, got: event.seq };
        }
        return { action: 'deliver' };
    }
    /**
     * Record that an event was delivered.
     *
     * Only ever called after the event was actually handled.
     */
    accept(event) {
        const key = scopeKey(event.scope);
        const current = this.#positions.get(key);
        if (current === undefined || event.seq > current) {
            this.#positions.set(key, event.seq);
        }
    }
    /** Forget a scope, so the next subscribe asks for everything. */
    reset(key) {
        this.#positions.delete(key);
    }
    /** Forget everything. */
    clear() {
        this.#positions.clear();
    }
}

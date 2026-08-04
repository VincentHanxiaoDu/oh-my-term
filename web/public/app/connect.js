/**
 * Opening the WebSocket a browser talks over.
 *
 * The socket carries exactly the messages the Unix socket carries, so nothing
 * above this file knows which transport it is on. That is the point of the
 * protocol being transport-independent, and it is why this module is small.
 */
/** How a reconnect backs off. */
export const RECONNECT_BASE_MS = 500;
/** The longest a reconnect waits. */
export const RECONNECT_MAX_MS = 30_000;
/**
 * How long to wait before the nth reconnect attempt.
 *
 * Exponential with a cap and jitter. The cap matters because a phone that
 * lost signal for an hour should still reconnect within seconds of getting it
 * back; the jitter matters because every client of an instance that restarted
 * would otherwise retry in the same millisecond and knock it over again.
 */
export function backoffMs(attempt, random = Math.random) {
    const exponential = Math.min(RECONNECT_BASE_MS * 2 ** attempt, RECONNECT_MAX_MS);
    // Full jitter: anywhere in [0, exponential], which spreads a thundering herd
    // far better than a small fraction around the target does.
    return Math.floor(random() * exponential);
}
/**
 * The WebSocket URL for an instance.
 *
 * Derived from the page's own scheme, so an instance served over TLS is
 * reached over `wss` — a hard-coded `ws` would be blocked by the browser as
 * mixed content, and the failure reads as "the server is down".
 */
export function socketUrl(base) {
    const url = new URL('/api/ws', base);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    return url.toString();
}
/**
 * Whether a close code means it is worth trying again.
 *
 * A rejected credential is not: retrying it produces the same rejection
 * forever and hides the one thing the user needs to be told.
 */
export function shouldReconnect(code) {
    // 1008 policy violation and 4001 (ours) both mean the credential was
    // refused. 1000 is a clean close nobody asked to undo.
    return code !== 1000 && code !== 1008 && code !== 4001;
}
/**
 * Open a connection.
 *
 * The token goes in a subprotocol rather than a query string: a query string
 * lands in access logs, browser history and any `Referer` the page sends, and
 * a browser's `WebSocket` cannot set headers.
 */
export function connect(options, handlers, factory = (u, p) => new WebSocket(u, p)) {
    const socket = factory(socketUrl(options.url), ['omt.v1', `omt.token.${options.token}`]);
    socket.onopen = () => handlers.onOpen?.();
    socket.onmessage = (event) => {
        if (typeof event.data !== 'string') {
            // Binary frames carry terminal bytes, which are handled by the byte path
            // rather than parsed as JSON. Attempting both is how a control message
            // gets applied twice.
            return;
        }
        try {
            handlers.onMessage(JSON.parse(event.data));
        }
        catch {
            // A message this build cannot parse is skipped rather than fatal: a
            // newer instance may send something this client does not know yet, and
            // dropping the connection over it would make every upgrade a outage.
        }
    };
    socket.onclose = (event) => handlers.onClose?.(event.code, shouldReconnect(event.code));
    return {
        send(message) {
            socket.send(JSON.stringify(message));
        },
        close() {
            socket.close(1000);
        },
    };
}

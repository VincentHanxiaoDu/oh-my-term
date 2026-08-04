/**
 * Getting notified when an agent needs you.
 *
 * The thing that makes this work without a cloud: **Web Push needs no push
 * server of your own.** The daemon generates a VAPID keypair and needs only
 * *outbound* HTTPS to the platform's push service, which never contacts it
 * back. So an instance on a laptop, reachable only over a tailnet, can still
 * wake a phone — and payloads are end-to-end encrypted to the browser's keys,
 * so the relay sees ciphertext.
 *
 * **Push is the wake-up; the socket is what you use once awake.** A
 * backgrounded WebSocket dies within seconds — equally true of a native app —
 * so anything that relies on the socket for delivery simply does not arrive
 * when the app is closed.
 */
/**
 * Whether this environment can receive push, and what to tell the user if not.
 *
 * The blockers are ordered by what the user has to do about them, not by how
 * the checks happen to run: telling somebody to grant permission when the real
 * problem is an untrusted certificate sends them to the wrong settings screen.
 */
export function canPush(env) {
    if (!env.isSecureContext) {
        return {
            available: false,
            blocker: {
                reason: 'insecure-origin',
                // Self-signed does not count: Safari refuses to register a service
                // worker without a trusted certificate, so there is no way around this
                // short of a real one.
                detail: 'push needs a trusted HTTPS origin — `tailscale cert` gives you one without exposing anything publicly',
            },
        };
    }
    if (!env.hasServiceWorker || !env.hasPushManager) {
        // On iOS this is what a Safari *tab* looks like: PushManager is simply not
        // exposed there, and every iOS browser is WebKit, so no browser escapes it.
        if (env.isIos && !env.isStandalone) {
            return {
                available: false,
                blocker: {
                    reason: 'needs-install',
                    detail: 'add omt to your Home Screen — iOS only allows push for installed apps',
                },
            };
        }
        return {
            available: false,
            blocker: {
                reason: 'unsupported',
                detail: 'this browser does not support Web Push',
            },
        };
    }
    if (env.isIos && !env.isStandalone) {
        return {
            available: false,
            blocker: {
                reason: 'needs-install',
                detail: 'add omt to your Home Screen — iOS only allows push for installed apps',
            },
        };
    }
    if (env.permission === 'denied') {
        return {
            available: false,
            blocker: {
                reason: 'denied',
                detail: 'notifications are blocked for this site in your browser settings',
            },
        };
    }
    return { available: true };
}
/**
 * Whether to raise a notification.
 *
 * Suppressed while the user is watching, which Claude Code does with a presence
 * file and for good reason: being buzzed about something you are actively
 * looking at is how notifications get turned off entirely.
 */
export function decide(context) {
    if (context.blocked === 0 || context.waiting.length === 0) {
        return { notify: false, because: 'nothing-waiting' };
    }
    if (context.focused) {
        return { notify: false, because: 'watching' };
    }
    const fresh = context.waiting.filter((id) => !context.alreadyTold.includes(id));
    if (fresh.length === 0) {
        // Re-notifying about the same card every time state changes is the fastest
        // way to teach somebody to ignore the app.
        return { notify: false, because: 'already-told' };
    }
    return {
        notify: true,
        title: `${context.sessionName} needs you`,
        body: fresh.length === 1
            ? 'An agent is waiting on an answer'
            : `${fresh.length} agents are waiting on an answer`,
    };
}
/** How many waiting threads to show on the app badge. */
export function badgeCount(blocked) {
    // `undefined` clears it. A badge of zero is a badge that says "nothing",
    // which is what no badge already says.
    return blocked > 0 ? blocked : undefined;
}

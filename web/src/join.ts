/**
 * Joining an instance: swapping an invite for a credential of this device's own.
 *
 * `docs/architecture/07-remote-protocol.md` §1.3. The invite link used to carry
 * a credential, so the QR on somebody's screen was permanent access for anybody
 * who ever saw it — a photograph, a shared screen, a shoulder — with nothing to
 * revoke short of the token itself.
 *
 * An invite expires, is exchanged once, and buys a credential belonging to the
 * device that exchanged it. Losing a phone is then something somebody can act
 * on without changing every other device's access.
 */

import { INVITE_PREFIX } from './instances.js'

/** Whether a token is an invite rather than a credential. */
export function isInvite(token: string): boolean {
  return token.startsWith(INVITE_PREFIX)
}

/** What this device tells an instance about itself. */
export function describeDevice(
  scope: { userAgent?: string; platform?: string } = navigator,
): { name: string; platform: string } {
  const agent = scope.userAgent ?? ''
  // A name somebody will recognise in a list of devices, from what the browser
  // will actually tell us. Not a fingerprint: this is a label for a human
  // deciding which credential to revoke.
  const kind = /iPhone/.test(agent)
    ? 'iPhone'
    : /iPad/.test(agent)
      ? 'iPad'
      : /Android/.test(agent)
        ? 'Android phone'
        : /Macintosh/.test(agent)
          ? 'Mac'
          : /Windows/.test(agent)
            ? 'Windows PC'
            : 'a browser'
  const browser = /CriOS|Chrome/.test(agent)
    ? 'Chrome'
    : /Firefox/.test(agent)
      ? 'Firefox'
      : /Safari/.test(agent)
        ? 'Safari'
        : 'browser'
  return { name: `${kind} · ${browser}`, platform: scope.platform ?? '' }
}

/**
 * Exchange an invite over a socket that has no credential yet.
 *
 * Its own short-lived connection rather than the main one: the exchange happens
 * before there is anything to connect *with*, and folding it into the client's
 * normal lifecycle would mean every reconnect carrying a branch for a state it
 * is only ever in once.
 */
export async function exchangeInvite(
  url: string,
  invite: string,
  device: { name: string; platform: string },
  open: (url: string) => WebSocket = (u) => new WebSocket(u),
): Promise<{ token: string; instance: { name: string } } | null> {
  const socket = open(`${url.replace(/^http/, 'ws')}/api/ws`)
  return await new Promise((resolve) => {
    // A bounded wait. An instance that never answers must not leave somebody
    // looking at a screen that says nothing forever.
    const giveUp = setTimeout(() => {
      socket.close()
      resolve(null)
    }, 15_000)

    const finish = (value: { token: string; instance: { name: string } } | null) => {
      clearTimeout(giveUp)
      socket.close()
      resolve(value)
    }

    socket.onopen = () => {
      socket.send(
        JSON.stringify({
          t: 'join_exchange',
          // A real uuid: the wire type is a device id, and a placeholder string
          // fails to deserialize — which closes the connection with no
          // explanation, so the join looks like the invite was rejected.
          request: { device: crypto.randomUUID(), n: 1 },
          invite,
          device,
        }),
      )
    }
    socket.onmessage = (event) => {
      if (typeof event.data !== 'string') {
        return
      }
      try {
        const message = JSON.parse(event.data) as {
          t: string
          token?: string
          instance?: { name: string }
        }
        if (message.t === 'join_credential' && message.token) {
          finish({ token: message.token, instance: message.instance ?? { name: url } })
          return
        }
        // Anything else on this socket is a refusal. Reported as "that did not
        // work" rather than retried: an invite is single-exchange, so a second
        // attempt with the same one cannot succeed.
        finish(null)
      } catch {
        finish(null)
      }
    }
    socket.onerror = () => finish(null)
    socket.onclose = () => finish(null)
  })
}

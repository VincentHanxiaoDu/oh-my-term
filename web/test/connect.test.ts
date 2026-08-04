import { describe, expect, it, vi } from 'vitest'
import {
  RECONNECT_MAX_MS,
  backoffMs,
  connect,
  shouldReconnect,
  socketUrl,
} from '../src/index.js'

describe('connecting', () => {
  it('follows the page scheme so TLS is not downgraded', () => {
    // A hard-coded `ws` on an https page is blocked as mixed content, and the
    // failure reads as "the server is down".
    expect(socketUrl('https://box.local:7717')).toBe('wss://box.local:7717/api/ws')
    expect(socketUrl('http://localhost:7717')).toBe('ws://localhost:7717/api/ws')
  })

  it('sends the token as a subprotocol, never in the URL', () => {
    // A query string lands in access logs, browser history and any Referer the
    // page sends — and a browser's WebSocket cannot set headers.
    const seen: { url?: string; protocols?: string[] } = {}
    const fake = { onopen: null, onmessage: null, onclose: null, send: () => {}, close: () => {} }
    connect(
      { url: 'https://box.local', token: 'omt_c_secret' },
      { onMessage: () => {} },
      (url, protocols) => {
        seen.url = url
        seen.protocols = protocols
        return fake as unknown as WebSocket
      },
    )
    expect(seen.url).not.toContain('omt_c_secret')
    expect(seen.protocols).toContain('omt.token.omt_c_secret')
  })
})

describe('reconnecting', () => {
  it('does not retry a rejected credential', () => {
    // Retrying produces the same rejection forever and hides the one thing the
    // user needs to be told.
    expect(shouldReconnect(1008)).toBe(false)
    expect(shouldReconnect(4001)).toBe(false)
    expect(shouldReconnect(1000)).toBe(false)
    expect(shouldReconnect(1006)).toBe(true)
  })

  it('backs off exponentially and stops growing', () => {
    // A phone that lost signal for an hour should reconnect within seconds of
    // getting it back, not keep doubling.
    const noJitter = () => 1
    expect(backoffMs(0, noJitter)).toBeLessThan(backoffMs(3, noJitter))
    expect(backoffMs(50, noJitter)).toBe(RECONNECT_MAX_MS)
  })

  it('spreads a thundering herd', () => {
    // Every client of an instance that restarted would otherwise retry in the
    // same millisecond and knock it over again.
    const low = backoffMs(5, () => 0.01)
    const high = backoffMs(5, () => 0.99)
    expect(low).toBeLessThan(high)
  })
})

describe('incoming messages', () => {
  function harness() {
    const messages: unknown[] = []
    let socket: Record<string, unknown> = {}
    const fake = {
      onopen: null as unknown,
      onmessage: null as unknown,
      onclose: null as unknown,
      send: vi.fn(),
      close: vi.fn(),
    }
    socket = fake
    const transport = connect(
      { url: 'http://x', token: 't' },
      { onMessage: (m) => messages.push(m) },
      () => fake as unknown as WebSocket,
    )
    return { messages, socket, transport }
  }

  it('parses a control message', () => {
    const { messages, socket } = harness()
    ;(socket.onmessage as (e: { data: unknown }) => void)({
      data: JSON.stringify({ t: 'goodbye', reason: 'shutting_down', detail: 'bye' }),
    })
    expect(messages).toHaveLength(1)
  })

  it('skips a message it cannot parse rather than dropping the connection', () => {
    // A newer instance may send something this client does not know yet, and
    // dropping the connection over it would make every upgrade an outage.
    const { messages, socket, transport } = harness()
    ;(socket.onmessage as (e: { data: unknown }) => void)({ data: '{ not json' })
    expect(messages).toHaveLength(0)
    expect(socket.close).not.toHaveBeenCalled()
    transport.close()
  })

  it('leaves binary frames to the byte path', () => {
    // Terminal bytes are not JSON. Attempting both is how a control message
    // gets applied twice.
    const { messages, socket } = harness()
    ;(socket.onmessage as (e: { data: unknown }) => void)({ data: new ArrayBuffer(8) })
    expect(messages).toHaveLength(0)
  })
})

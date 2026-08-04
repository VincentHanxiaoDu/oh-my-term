import { describe, expect, it } from 'vitest'
import {
  type Deliverable,
  InstanceClient,
  type Interaction,
  type OmtEvent,
  type ServerMessage,
  isAnswerable,
} from '../src/index.js'

function client(): InstanceClient {
  const c = new InstanceClient('device-1')
  c.receive({
    t: 'welcome',
    proto: 1,
    role: 'operator',
    catalog_hash: 'abc',
    capabilities: ['session.list', 'interaction.resolve'],
  })
  return c
}

function interaction(
  id: string,
  deliverable: Deliverable,
  state: Interaction['state'] = { state: 'open' },
): Interaction {
  return { id, session: 's1', binding: 'b1', deliverable, state, opened_at: '2026-01-01T00:00:00Z' }
}

function interactionEvent(seq: number, i: Interaction): ServerMessage {
  const event: OmtEvent = {
    scope: { kind: 'session', session: 's1' },
    seq,
    ts: '2026-01-01T00:00:00Z',
    source: 'hook',
    payload: { interaction: i },
  }
  return { t: 'event', event }
}

describe('capability intersection', () => {
  it('reports only what this instance actually offers', () => {
    // A phone attached to several instances on different versions renders what
    // each supports and greys out the rest, rather than erroring on the first
    // mismatch.
    const c = client()
    expect(c.can('session.list')).toBe(true)
    expect(c.can('future.capability')).toBe(false)
  })

  it('offers nothing before a welcome arrives', () => {
    // Otherwise a surface draws buttons during the handshake.
    const c = new InstanceClient('device-1')
    expect(c.can('session.list')).toBe(false)
    expect(c.connection.state).toBe('disconnected')
  })

  it('reports a refusal with the reason a human can act on', () => {
    const c = new InstanceClient('device-1')
    c.receive({ t: 'goodbye', reason: 'unauthorized', detail: 'the token was rejected' })
    expect(c.connection).toEqual({
      state: 'refused',
      reason: 'unauthorized',
      detail: 'the token was rejected',
    })
  })
})

describe('answerability', () => {
  it('reads from the deliverable, never from the state', () => {
    // An open card is not necessarily one omt can answer. Offering a button for
    // one it cannot means the user finds out by the wrong option being chosen.
    const c = client()
    c.receive(interactionEvent(1, interaction('i1', { kind: 'native' })))
    c.receive(interactionEvent(2, interaction('i2', { kind: 'none', reason: 'no responder' })))
    expect(c.openInteractions()).toHaveLength(2)
    expect(c.answerable().map((i) => i.id)).toEqual(['i1'])
  })

  it('says why a card cannot be answered here', () => {
    // A card the user can see but not act on has to say why and point at the
    // terminal, or it reads as a broken button.
    const c = client()
    const blocked = interaction('i1', { kind: 'none', reason: 'this agent has no responder' })
    c.receive(interactionEvent(1, blocked))
    expect(c.whyNotAnswerable(blocked)).toBe('this agent has no responder')
  })

  it('says so when somebody else already answered', () => {
    const c = client()
    const done = interaction('i1', { kind: 'native' }, { state: 'resolved', by: 'phone' })
    c.receive(interactionEvent(1, done))
    expect(c.whyNotAnswerable(done)).toBe('it has already been answered')
    expect(c.openInteractions()).toHaveLength(0)
  })

  it('treats a synthetic delivery as answerable but gated', () => {
    const d: Deliverable = { kind: 'synthetic', requires_token: true }
    expect(isAnswerable(d)).toBe(true)
    expect(d.requires_token).toBe(true)
  })
})

describe('event application', () => {
  it('a later transition replaces the whole interaction', () => {
    // One event carrying the whole object rather than four narrow ones: a
    // client that missed an earlier transition still ends up correct, and
    // missing one is the normal case on a phone.
    const c = client()
    c.receive(interactionEvent(1, interaction('i1', { kind: 'native' })))
    c.receive(
      interactionEvent(2, interaction('i1', { kind: 'native' }, { state: 'resolved', by: 'laptop' })),
    )
    expect(c.openInteractions()).toHaveLength(0)
  })

  it('a duplicate event does not apply twice', () => {
    const c = client()
    c.receive(interactionEvent(1, interaction('i1', { kind: 'native' })))
    expect(c.receive(interactionEvent(1, interaction('i1', { kind: 'native' })))).toBe('skip')
  })

  it('a gap is reported rather than silently applied', () => {
    const c = client()
    c.receive(interactionEvent(1, interaction('i1', { kind: 'native' })))
    expect(c.receive(interactionEvent(5, interaction('i2', { kind: 'native' })))).toBe('gap')
    expect(c.openInteractions().map((i) => i.id)).toEqual(['i1'])
  })

  it('a resync drops the position so the next reconnect asks afresh', () => {
    // Keeping it would have the client resume from a point that no longer
    // exists.
    const c = client()
    c.receive(interactionEvent(1, interaction('i1', { kind: 'native' })))
    expect(c.resume.since()).toEqual({ s1: 1 })
    expect(
      c.receive({
        t: 'resync',
        scope_key: 's1',
        reason: 'window_exceeded',
        dropped: 400,
        from: 500,
      }),
    ).toBe('resync-needed')
    expect(c.resume.since()).toEqual({})
  })
})

describe('reconnection', () => {
  it('sends the positions it already holds', () => {
    const c = client()
    c.receive(interactionEvent(1, interaction('i1', { kind: 'native' })))
    c.disconnect()
    const messages = c.hello('omt_c_token')
    expect(messages[0]).toMatchObject({ t: 'hello', client: 'web', token: 'omt_c_token' })
    expect(messages[1]).toEqual({ t: 'subscribe', since: { s1: 1 } })
  })

  it('omits the token entirely when there is none', () => {
    // Sending `token: undefined` is a different message from sending none, and
    // a server checking for the field's presence would see one.
    const c = new InstanceClient('device-1')
    expect(c.hello()[0]).not.toHaveProperty('token')
  })

  it('mints request ids that do not repeat', () => {
    // Minted client-side at intent time, because a disconnected client cannot
    // ask for one — which is exactly when it needs to retry safely later.
    const c = client()
    const a = c.request()
    const b = c.request()
    expect(a.n).not.toBe(b.n)
    expect(a.device).toBe('device-1')
  })
})

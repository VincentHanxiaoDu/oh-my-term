import { describe, expect, it, vi } from 'vitest'
import { Store, type ClientMessage, type ServerMessage } from '../src/index.js'

function harness(capabilities = ['workspace.list', 'session.list', 'agent.threads']) {
  const sent: ClientMessage[] = []
  const store = new Store('device-1', { send: (m) => sent.push(m) })
  store.connect('omt_c_token')
  store.receive({
    t: 'welcome',
    proto: 1,
    role: 'operator',
    catalog_hash: 'abc',
    capabilities,
  })
  return { store, sent }
}

/** Answer the most recent call with a result. */
function reply(store: Store, sent: ClientMessage[], output: unknown, status: 'ok' | 'err' = 'ok') {
  const call = [...sent].reverse().find((m) => m.t === 'call')
  if (!call || call.t !== 'call') throw new Error('no call was sent')
  const message: ServerMessage =
    status === 'ok'
      ? { t: 'result', request: call.request, status: 'ok', output }
      : {
          t: 'result',
          request: call.request,
          status: 'err',
          error: { code: 'not_found', message: 'no such thing' },
        }
  store.receive(message)
}

describe('the store', () => {
  it('refuses a capability this instance does not offer, without sending', () => {
    // A call that went out and came back "unknown capability" costs a round
    // trip to learn something the welcome already said.
    const { store, sent } = harness(['workspace.list'])
    const before = sent.length
    return expect(store.call('git.status', 'wksp_x')).rejects.toMatchObject({
      code: 'unsupported',
    }).then(() => {
      expect(sent.length).toBe(before)
    })
  })

  it('resolves a call with its output', async () => {
    const { store, sent } = harness()
    const promise = store.call('workspace.list')
    reply(store, sent, { workspaces: [{ id: 'w1', root: '/w', name: 'w', sessions: 2 }] })
    await expect(promise).resolves.toMatchObject({ workspaces: expect.any(Array) })
  })

  it('rejects a call with the error the server gave', async () => {
    const { store, sent } = harness()
    const promise = store.call('session.list')
    reply(store, sent, null, 'err')
    await expect(promise).rejects.toMatchObject({
      capability: 'session.list',
      code: 'not_found',
    })
  })

  it('ignores a result for a request it never made', () => {
    // Acting on it would apply somebody else's answer.
    const { store } = harness()
    expect(() =>
      store.receive({
        t: 'result',
        request: { device: 'someone-else', n: 99 },
        status: 'ok',
        output: {},
      }),
    ).not.toThrow()
  })

  it('does not settle the same call twice', async () => {
    // A duplicate result after a reconnect would resolve a promise that had
    // already been handled.
    const { store, sent } = harness()
    const promise = store.call('workspace.list')
    reply(store, sent, { workspaces: [] })
    await promise
    expect(() => reply(store, sent, { workspaces: [] })).not.toThrow()
  })

  it('tells the user when it missed updates rather than continuing quietly', () => {
    // A client that silently carried on would be showing stale state as if it
    // were current.
    const { store } = harness()
    store.receive({
      t: 'resync',
      scope_key: 's1',
      reason: 'window_exceeded',
      dropped: 400,
      from: 500,
    })
    expect(store.state.problems[0]?.message).toContain('missed some updates')
    expect(store.state.problems[0]?.transient).toBe(true)
  })

  it('marks a rejected credential as not transient', () => {
    // It will not fix itself, and a UI that showed "reconnecting…" forever
    // would hide the one thing the user has to act on.
    const { store } = harness()
    store.receive({ t: 'goodbye', reason: 'unauthorized', detail: 'the token was rejected' })
    expect(store.state.problems[0]).toMatchObject({ transient: false })
    expect(store.state.connection).toBe('refused')
    expect(store.state.refusal).toContain('rejected')
  })

  it('counts only the cards it can actually answer', () => {
    // An open card is not necessarily one omt can answer, and a header that
    // counted both would send the user looking for a button that is not there.
    const { store } = harness()
    const card = (id: string, deliverable: Interaction['deliverable']) => ({
      t: 'event' as const,
      event: {
        scope: { kind: 'session' as const, session: 's1' },
        seq: id === 'i1' ? 1 : 2,
        ts: '2026-01-01T00:00:00Z',
        source: 'hook',
        payload: {
          interaction: {
            id,
            session: 's1',
            binding: 'b1',
            deliverable,
            state: { state: 'open' as const },
            opened_at: '2026-01-01T00:00:00Z',
          },
        },
      },
    })
    store.receive(card('i1', { kind: 'native' }))
    store.receive(card('i2', { kind: 'none', reason: 'no responder' }))

    expect(store.state.open).toHaveLength(2)
    expect(store.state.needsYou).toBe(1)
    expect(store.answerable().map((i) => i.id)).toEqual(['i1'])
  })

  it('notifies subscribers when anything changes', () => {
    const { store } = harness()
    const seen = vi.fn()
    const unsubscribe = store.subscribe(seen)
    store.receive({ t: 'goodbye', reason: 'shutting_down', detail: 'bye' })
    expect(seen).toHaveBeenCalled()
    unsubscribe()
  })

  it('stops notifying after unsubscribe', () => {
    const { store } = harness()
    const seen = vi.fn()
    store.subscribe(seen)()
    store.receive({ t: 'goodbye', reason: 'shutting_down', detail: 'bye' })
    expect(seen).not.toHaveBeenCalled()
  })

  it('builds the subagent grid blocked-first from what the server said', async () => {
    const { store, sent } = harness()
    const promise = store.loadThreads('sess_1')
    reply(store, sent, {
      threads: [
        { id: 'main', is_subagent: false, state: 'working', open_interactions: [] },
        { id: 'sub1', is_subagent: true, state: 'blocked', open_interactions: ['i1'] },
        { id: 'sub2', is_subagent: true, state: 'idle', open_interactions: [] },
      ],
      blocked: 1,
    })
    await promise

    expect(store.gridFor('sess_1').map((c) => c.thread.id)).toEqual(['sub1', 'main', 'sub2'])
    expect(store.summaryFor('sess_1')).toBe('1 of 3 need you')
  })

  it('a session with no threads loaded renders empty rather than throwing', () => {
    const { store } = harness()
    expect(store.gridFor('unknown')).toEqual([])
    expect(store.summaryFor('unknown')).toBe('nothing running')
  })

  it('a failed refresh surfaces as a problem rather than an exception', async () => {
    const { store, sent } = harness()
    const promise = store.refresh()
    reply(store, sent, null, 'err')
    await promise
    expect(store.state.problems.length).toBeGreaterThan(0)
  })

  it('a problem can be dismissed', () => {
    const { store } = harness()
    store.receive({ t: 'goodbye', reason: 'shutting_down', detail: 'bye' })
    expect(store.state.problems).toHaveLength(1)
    store.dismiss(0)
    expect(store.state.problems).toHaveLength(0)
  })
})

// Imported late so the type is in scope for the helper above.
import type { Interaction } from '../src/index.js'

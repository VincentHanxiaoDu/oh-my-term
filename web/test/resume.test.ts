import { describe, expect, it } from 'vitest'
import { INSTANCE_SCOPE_KEY, type OmtEvent, ResumeState, scopeKey } from '../src/index.js'

function event(session: string, seq: number): OmtEvent {
  return {
    scope: { kind: 'session', session },
    seq,
    ts: '2026-01-01T00:00:00Z',
    source: 'hook',
    payload: {},
  }
}

describe('resume positions', () => {
  it('advances only for events that were delivered', () => {
    // The failure this prevents: a position recorded for an event the client
    // dropped means the next reconnect resumes past it and nobody notices.
    const r = new ResumeState()
    const e = event('s1', 1)
    expect(r.position('s1')).toBeUndefined()
    expect(r.inspect(e).action).toBe('deliver')
    expect(r.position('s1')).toBeUndefined()
    r.accept(e)
    expect(r.position('s1')).toBe(1)
  })

  it('skips an event it has already seen', () => {
    // Duplicates after a reconnect. Rendering one again shows the same terminal
    // output twice, which is indistinguishable from the program printing it.
    const r = new ResumeState()
    r.accept(event('s1', 5))
    expect(r.inspect(event('s1', 5))).toEqual({ action: 'skip', why: 'already-seen' })
    expect(r.inspect(event('s1', 3))).toEqual({ action: 'skip', why: 'already-seen' })
  })

  it('reports a gap rather than continuing quietly', () => {
    // Continuing leaves the client believing it is current when it is not.
    const r = new ResumeState()
    r.accept(event('s1', 1))
    expect(r.inspect(event('s1', 4))).toEqual({ action: 'gap', expected: 2, got: 4 })
  })

  it('accepts the next event in sequence', () => {
    const r = new ResumeState()
    r.accept(event('s1', 1))
    expect(r.inspect(event('s1', 2)).action).toBe('deliver')
  })

  it('tracks each session independently', () => {
    // A shared position would make one session's traffic advance another's, and
    // the quiet one would silently skip on reconnect.
    const r = new ResumeState()
    r.accept(event('s1', 10))
    expect(r.inspect(event('s2', 1)).action).toBe('deliver')
    expect(r.position('s2')).toBeUndefined()
  })

  it('produces a resume map covering every stream it follows', () => {
    const r = new ResumeState()
    r.accept(event('s1', 3))
    r.accept(event('s2', 7))
    expect(r.since()).toEqual({ s1: 3, s2: 7 })
  })

  it('asks for everything again after a reset', () => {
    const r = new ResumeState()
    r.accept(event('s1', 9))
    r.reset('s1')
    expect(r.since()).toEqual({})
    expect(r.inspect(event('s1', 1)).action).toBe('deliver')
  })

  it('never moves a position backwards', () => {
    // An out-of-order accept would make the client re-request events it has.
    const r = new ResumeState()
    r.accept(event('s1', 5))
    r.accept(event('s1', 2))
    expect(r.position('s1')).toBe(5)
  })

  it('keys every scope into one uniformly typed map', () => {
    // So a client does not have to discriminate a union to build a resume map.
    expect(scopeKey({ kind: 'session', session: 's1' })).toBe('s1')
    expect(scopeKey({ kind: 'workspace', workspace: 'w1' })).toBe('w1')
    expect(scopeKey({ kind: 'instance' })).toBe(INSTANCE_SCOPE_KEY)
  })
})

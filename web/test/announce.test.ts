import { describe, expect, it } from 'vitest'

import { announcement } from '../src/app'

const row = (id: string, state: string, title = id) => ({
  id,
  title,
  state,
  workspace: 'w',
})

describe('what a screen reader is told', () => {
  it('says nothing when nothing changed', () => {
    // A region that speaks on every render teaches somebody to tune it out,
    // and then it is not there for the one that mattered.
    const rows = [row('a', 'idle'), row('b', 'working')]
    expect(announcement(rows, rows)).toBeNull()
  })

  it('announces a session that has just started needing a human', () => {
    // The one event that stops work, and the entire reason this surface
    // exists.
    const before = [row('a', 'working', 'api')]
    const after = [row('a', 'blocked', 'api')]
    expect(announcement(before, after)).toEqual({
      message: 'api needs you',
      urgent: true,
    })
  })

  it('does not announce one that was already blocked', () => {
    // Otherwise every poll repeats it and the region becomes a metronome.
    const rows = [row('a', 'blocked')]
    expect(announcement(rows, rows)).toBeNull()
  })

  it('counts rather than listing when several arrive at once', () => {
    // Three separate announcements in a row is noise somebody turns off.
    const before = [row('a', 'working'), row('b', 'working')]
    const after = [row('a', 'blocked'), row('b', 'blocked')]
    expect(announcement(before, after)?.message).toBe('2 sessions need you')
  })

  it('announces a newly blocked session even while another is outstanding', () => {
    // The case a naive "is anything blocked" check gets wrong: something new
    // needs a human and nothing is said, because something already did.
    const before = [row('a', 'blocked'), row('b', 'working', 'web')]
    const after = [row('a', 'blocked'), row('b', 'blocked', 'web')]
    expect(announcement(before, after)?.message).toBe('web needs you')
  })

  it('says nothing when a session stops needing a human', () => {
    // Being told an answer was accepted is not urgent, and the roster shows it.
    const before = [row('a', 'blocked')]
    const after = [row('a', 'working')]
    expect(announcement(before, after)).toBeNull()
  })

  it('is urgent, because a polite region waits for a pause', () => {
    // A blocked agent is not something to mention at the next convenient
    // moment; it is the thing that stopped.
    const before = [row('a', 'working')]
    const after = [row('a', 'blocked')]
    expect(announcement(before, after)?.urgent).toBe(true)
  })
})

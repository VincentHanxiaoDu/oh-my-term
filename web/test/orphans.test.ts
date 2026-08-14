import { describe, expect, it } from 'vitest'
import { isOrphaned, waitingLine } from '../src/federation.js'
import { en, zhHans } from '../src/messages.js'

/** A session, as the roster sees one. */
function session(state: string) {
  return { id: `s${state}`, state }
}

describe('a session whose process is gone', () => {
  it('is neither running nor an error', () => {
    // An orphan is a session omt restored from a snapshot: the scrollback came
    // back and the pty did not. Counting it as running is a headline the rows
    // underneath contradict; counting it as an error is a red screen for a
    // reboot.
    expect(isOrphaned('orphaned')).toBe(true)
    for (const other of ['running', 'starting', 'working', 'blocked', 'exited']) {
      expect(isOrphaned(other), other).toBe(false)
    }
  })
})

describe('what the roster says it is holding', () => {
  it('counts the living and the dead separately', () => {
    // Folding orphans into the running count is the bug this line already had
    // once — "7 running" over seven rows that all read "orphaned". Folding
    // them into nothing leaves a roster whose length nobody can account for.
    const said = waitingLine(9, 0, 2, 7)
    expect(said).toContain('2 running')
    expect(said).toContain('7 ended')
  })

  it('says nothing about the dead when there are none', () => {
    // A count of zero is noise on the one line somebody reads at a glance.
    const said = waitingLine(2, 0, 2, 0)
    expect(said).toContain('2 running')
    expect(said).not.toMatch(/ended|0/)
  })

  it('keeps that sentence short enough for a phone bar', () => {
    // The long version — "3 sessions ended with omt" — was truncated to
    // "3 sessions end…" in the app bar, which is a headline that stops
    // mid-word. The rows underneath already say "orphaned".
    expect(waitingLine(3, 0, 0, 3).length).toBeLessThanOrEqual(30)
    expect(waitingLine(16, 0, 2, 14).length).toBeLessThanOrEqual(40)
  })

  it('has one plain sentence for a roster that is all corpses', () => {
    // What somebody comes back to after a reboot. "16 open" over sixteen rows
    // that all read "orphaned" is the same failure as "7 running" over them.
    const said = waitingLine(16, 0, 0, 16)
    expect(said).toContain('16')
    // Not "16 open", which reads as sixteen things somebody could go and use.
    expect(said).not.toContain('open')
    // And it does not claim any of them is running.
    expect(said).toContain('nothing running')
  })

  it('still puts what needs a human first, whatever else is there', () => {
    // The one thing the roster exists to say. A hundred orphans do not get to
    // push it off the line.
    expect(waitingLine(100, 1, 0, 99)).toBe('1 waiting on you')
  })

  it('says nothing running when there is nothing at all', () => {
    expect(waitingLine(0, 0, 0, 0)).toBe('nothing running')
  })
})

describe('the set that clear-all acts on', () => {
  it('never contains a session that is running', () => {
    // The one code path in this client that closes several things at once.
    // `session.close` on a running session kills a process.
    const roster = [
      session('running'),
      session('orphaned'),
      session('blocked'),
      session('orphaned'),
      session('starting'),
      session('exited'),
    ]
    const dead = roster.filter((s) => isOrphaned(s.state))
    expect(dead).toHaveLength(2)
    for (const s of dead) {
      expect(s.state).toBe('orphaned')
    }
  })

  it('is empty when nothing has ended, so the control is absent', () => {
    const roster = [session('running'), session('blocked')]
    expect(roster.filter((s) => isOrphaned(s.state))).toEqual([])
  })
})

describe('what an orphan is told about itself', () => {
  it('explains the state rather than naming it, in both languages', () => {
    // "orphaned" is omt's word for it. What somebody needs to know is that
    // the output is real and nothing is listening.
    for (const table of [en, zhHans]) {
      const said = table['orphan.explain']
      expect(said).toBeTruthy()
      expect(said).not.toContain('orphan')
    }
    expect(zhHans['orphan.explain']).not.toBe(en['orphan.explain'])
  })

  it('names the count on the control itself', () => {
    // The number is the confirmation. A dialog that says "are you sure" says
    // nothing and is answered yes.
    expect(en['orphan.clear'](7)).toContain('7')
    expect(en['orphan.clear'](1)).toContain('1')
    // Partial by design: a key with no entry falls back to English.
    expect(zhHans['orphan.clear']?.(7)).toContain('7')
  })

  it('offers both a way back and a way out', () => {
    // Restart, because the session is usually still wanted; close, because
    // sometimes it is not. Offering only one of them is how sixteen of these
    // accumulate.
    expect(en['orphan.restart']).toBeTruthy()
    expect(en['orphan.close']).toBeTruthy()
    expect(zhHans['orphan.restart']).toBeTruthy()
    expect(zhHans['orphan.close']).toBeTruthy()
  })
})

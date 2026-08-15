import { describe, expect, it } from 'vitest'
import { attention, isOrphaned, stateWord, waitingLine } from '../src/federation.js'
import { rowMeta } from '../src/app.js'
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

describe('both surfaces call a state the same thing', () => {
  it('says waiting on you rather than blocked', () => {
    // A person looking at the phone and then at the terminal must not have to
    // work out that "blocked" and "waiting on you" are the same thing. The
    // wire carries the state; what to call it was a decision each surface was
    // making separately, and differently.
    expect(stateWord('blocked')).toBe('waiting on you')
    expect(stateWord('working')).toBe('working')
  })

  it('separates done from idle by whether anybody looked', () => {
    // Not on the wire, and not the agent's business: an agent is idle either
    // way. An agent that finished while you were elsewhere is the one thing
    // this product exists to surface.
    expect(stateWord('idle', false)).toBe('done')
    expect(stateWord('idle', true)).toBe('idle')
  })

  it('never says unknown', () => {
    expect(stateWord('unknown')).not.toContain('unknown')
  })

  it('passes through a state it has no word for rather than inventing one', () => {
    // A new state from a newer instance is a word omt does not have. Printing
    // it is honest; guessing at a friendlier one is not.
    expect(stateWord('quiescing')).toBe('quiescing')
  })
})

describe('what this device has looked at', () => {
  it('is a fact about the device, not about the agent', () => {
    // A session you read on your laptop is not one you have seen on your
    // phone. A flag shared through the instance would get that wrong in the
    // direction that loses things: the phone would call an agent's result
    // "idle" because somebody else had looked at it.
    expect(rowMeta({ state: 'idle' }, false, undefined, [])).toContain('done')
    expect(rowMeta({ state: 'idle' }, true, undefined, [])).toContain('idle')
  })

  it('says what is waiting whether or not anybody looked', () => {
    expect(rowMeta({ state: 'blocked' }, true, undefined, [])).toContain('waiting on you')
  })
})

describe('what the roster puts first', () => {
  it('puts a finished result above something still running', () => {
    // Reads backwards until you ask what each row wants. A session still
    // working wants nothing from anybody; a finished one is the whole reason
    // somebody opened the list. Sorting by "is it moving" puts the rows that
    // need nothing at the top.
    expect(attention('idle', false)).toBeLessThan(attention('working'))
  })

  it('puts what cannot continue without you above everything', () => {
    for (const other of ['working', 'running', 'idle', 'orphaned', 'exited']) {
      expect(attention('blocked')).toBeLessThan(attention(other))
    }
    expect(attention('blocked')).toBeLessThan(attention('idle', false))
  })

  it('puts a seen idle session below a working one', () => {
    // Once you have looked at it, it is the least urgent thing on the screen.
    expect(attention('idle', true)).toBeGreaterThan(attention('working'))
  })

  it('puts a state it cannot describe last rather than first', () => {
    // A row omt cannot account for must not be the first thing a person
    // reads, and must not be hidden either.
    expect(attention('quiescing')).toBeGreaterThan(attention('idle', true))
  })
})

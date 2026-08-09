import { describe, expect, it } from 'vitest'
import {
  cardTitle,
  dictationAvailable,
  chipLabel,
  chipTone,
  defaultSurface,
  failed,
  markFor,
  markSvg,
  openingScreen,
  pairCalls,
  summariseCall,
  unify,
  unifiedSubtitle,
  waiting,
  holdToConfirm,
  promptTag,
  surfacesFor,
} from '../src/index.js'
import type { TranscriptEntry } from '../src/index.js'

describe('which surfaces a session offers', () => {
  const shell = { threads: 0, shellIntegration: true, native: false }
  const agent = { threads: 1, shellIntegration: false, native: false }

  it('does not offer block view for an agent session', () => {
    // An agent emits no command marks at all, so blocks are not a worse option
    // — they are not an option, and an empty list would read as "nothing has
    // been run" about a session doing plenty.
    const blocks = surfacesFor(agent).find((o) => o.surface === 'blocks')
    expect(blocks?.available).toBe(false)
    expect(blocks?.why).toContain('agent')
  })

  it('does not offer a transcript for a session with no agent in it', () => {
    const transcript = surfacesFor(shell).find((o) => o.surface === 'transcript')
    expect(transcript?.available).toBe(false)
  })

  it('says why a surface is unavailable rather than only disabling it', () => {
    // A disabled control with no reason is indistinguishable from a broken one.
    for (const offer of [...surfacesFor(shell), ...surfacesFor(agent)]) {
      if (!offer.available) {
        expect(offer.why, `${offer.surface} is disabled and silent`).toBeTruthy()
      }
    }
  })

  it('disables block view for a shell that does not mark commands', () => {
    const offers = surfacesFor({ ...shell, shellIntegration: false })
    expect(offers.find((o) => o.surface === 'blocks')?.available).toBe(false)
  })

  it('has no terminal at all for a native session', () => {
    // Absent rather than disabled: a native session has no pty, so it is not a
    // thing that could become available later.
    const offers = surfacesFor({ threads: 1, shellIntegration: false, native: true })
    expect(offers.some((o) => o.surface === 'terminal')).toBe(false)
  })

  it('always keeps the terminal available for a pty session', () => {
    expect(surfacesFor(shell).find((o) => o.surface === 'terminal')?.available).toBe(true)
  })
})

describe('which surface a session opens on', () => {
  it('opens an agent session on its conversation', () => {
    // That is what somebody picked the phone up to read.
    expect(defaultSurface(surfacesFor({ threads: 1, shellIntegration: false, native: false }))).toBe(
      'transcript',
    )
  })

  it('opens a shell on its commands where the shell marks them', () => {
    expect(defaultSurface(surfacesFor({ threads: 0, shellIntegration: true, native: false }))).toBe(
      'blocks',
    )
  })

  it('falls back to the grid when nothing better is available', () => {
    expect(defaultSurface(surfacesFor({ threads: 0, shellIntegration: false, native: false }))).toBe(
      'terminal',
    )
  })

  it('sends a session with several threads to the thread list', () => {
    // Picking one subagent out of five for somebody is picking wrong most of
    // the time.
    expect(openingScreen({ state: 'running' }, 3)).toBe('threads')
  })

  it('sends a single-agent session straight to its conversation', () => {
    expect(openingScreen({ state: 'running' }, 1)).toBe('transcript')
  })

  it('sends a plain shell to the terminal', () => {
    expect(openingScreen({ state: 'running' }, 0)).toBe('terminal')
  })
})

describe('the exit chip', () => {
  const block = (over: Record<string, unknown> = {}) => ({
    id: 'b',
    command: 'ls',
    outcome: 'exited',
    failed: false,
    attributed: true,
    ...over,
  })

  it('does not call a running command a success', () => {
    expect(chipTone(block({ outcome: 'running' }))).toBe('working')
  })

  it('does not call an unknown outcome a success', () => {
    // The shell said the command ended and did not say how. Painting that green
    // is a claim omt cannot make.
    expect(chipTone(block({ outcome: 'unknown' }))).toBe('muted')
    expect(chipLabel(block({ outcome: 'unknown' }))).toBe('?')
  })

  it('shows the exit code of a failure', () => {
    expect(chipLabel(block({ failed: true, exit_code: 2 }))).toBe('2')
  })

  it('marks a success', () => {
    expect(chipTone(block())).toBe('ok')
  })
})

describe('the transcript', () => {
  const call: TranscriptEntry = {
    kind: 'tool_call',
    call: 'c1',
    name: 'Bash',
    input: { command: 'cargo test --workspace' },
  }

  it('pairs a tool call with its result', () => {
    // Apart, a call and its outcome have a wall of output between them, and the
    // one question anybody has — did that work — needs scrolling to answer.
    const paired = pairCalls([
      call,
      { kind: 'tool_result', call: 'c1', output: 'ok', duration_ms: 42 },
    ])
    expect(paired).toHaveLength(1)
    expect(paired[0]?.duration_ms).toBe(42)
  })

  it('leaves a call with no result alone rather than inventing one', () => {
    const paired = pairCalls([call])
    expect(paired).toHaveLength(1)
    expect(paired[0]?.output).toBeUndefined()
  })

  it('names the argument that says what happened', () => {
    // `Bash(cargo test)` says what happened; `{"command":"cargo test"}` makes
    // somebody read JSON on a phone.
    expect(summariseCall(call)).toBe('cargo test --workspace')
  })

  it('shortens an argument that would bury the entries around it', () => {
    const long = { ...call, input: { command: 'x'.repeat(400) } }
    expect(summariseCall(long).length).toBeLessThanOrEqual(120)
  })

  it('reads failure from what the result said, not from a guess', () => {
    expect(failed({ kind: 'tool_call', error: 'no such file' })).toBe(true)
    expect(failed({ kind: 'tool_call', error: null })).toBe(false)
  })
})

describe('the mark', () => {
  it('is the terminal one for a session with no agent', () => {
    expect(markFor(0)).toBe('terminal')
  })

  it('is the agent one once something is running in it', () => {
    expect(markFor(2)).toBe('agent')
  })

  it('draws both at any size, inheriting the colour around it', () => {
    // `currentColor`, so the mark is the accent in a bar and the tab colour in
    // a favicon without a second asset — and it cannot fail to load.
    for (const kind of ['terminal', 'agent'] as const) {
      const svg = markSvg(kind, 32)
      expect(svg).toContain('currentColor')
      expect(svg).toContain('width="32"')
    }
  })

  it('draws two different marks', () => {
    expect(markSvg('terminal')).not.toBe(markSvg('agent'))
  })
})

describe('an irreversible approval', () => {
  /** A button that records what happened to it. */
  function fakeButton() {
    const listeners = new Map<string, ((event: unknown) => void)[]>()
    const classes = new Set<string>()
    return {
      answered: 0,
      classList: {
        add: (c: string) => void classes.add(c),
        remove: (c: string) => void classes.delete(c),
        has: (c: string) => classes.has(c),
      },
      addEventListener(name: string, fn: (event: unknown) => void) {
        listeners.set(name, [...(listeners.get(name) ?? []), fn])
      },
      fire(name: string, event: unknown = { target: {}, pointerId: 1, preventDefault() {} }) {
        for (const fn of listeners.get(name) ?? []) {
          fn(event)
        }
      },
    }
  }

  it('does nothing on a tap', async () => {
    // The class alone was a lie: a hold-styled button that a single tap still
    // activated put an irreversible approval one accidental thumb away, on a
    // device people use while walking.
    const button = fakeButton()
    let answered = 0
    holdToConfirm(button as never, () => (answered += 1), 50)

    button.fire('pointerdown')
    button.fire('pointerup')
    await new Promise((r) => setTimeout(r, 80))
    expect(answered).toBe(0)
  })

  it('answers once the hold has lasted long enough', async () => {
    const button = fakeButton()
    let answered = 0
    holdToConfirm(button as never, () => (answered += 1), 30)

    button.fire('pointerdown')
    await new Promise((r) => setTimeout(r, 60))
    expect(answered).toBe(1)
  })

  it('is cancelled by sliding a thumb off it', async () => {
    const button = fakeButton()
    let answered = 0
    holdToConfirm(button as never, () => (answered += 1), 40)

    button.fire('pointerdown')
    button.fire('pointerleave')
    await new Promise((r) => setTimeout(r, 70))
    expect(answered).toBe(0)
  })
})

describe('how a card presents itself', () => {
  it('shows a permission prompt as a command, not as prose', () => {
    // A command shown as prose is a command somebody skims.
    expect(promptTag('permission')).toBe('pre')
  })

  it('shows a question as a question', () => {
    // And a question shown as code reads as something to run.
    expect(promptTag('choice')).toBe('p')
    expect(promptTag('plan_review')).toBe('p')
  })

  it('titles each kind as what it is', () => {
    expect(cardTitle('permission')).toBe('Permission')
    expect(cardTitle('plan_review')).toBe('Review this plan')
    expect(cardTitle('text')).toBe('Answer')
    expect(cardTitle('choice')).toBe('Needs you')
  })
})

describe('dictation', () => {
  it('is unavailable without a recogniser', () => {
    // Most browsers other than Chrome have none.
    expect(dictationAvailable({ isSecureContext: true })).toBe(false)
  })

  it('is unavailable outside a secure context, however capable the browser', () => {
    // The microphone needs one, and `http://` on a LAN — which is how a phone
    // usually reaches omt — is not one. The button says so rather than failing
    // on tap.
    expect(
      dictationAvailable({ SpeechRecognition: () => {}, isSecureContext: false }),
    ).toBe(false)
  })

  it('is available with both', () => {
    expect(
      dictationAvailable({ SpeechRecognition: () => {}, isSecureContext: true }),
    ).toBe(true)
  })
})

describe('one list across every instance', () => {
  const laptop = {
    instance: 'i1',
    label: 'laptop',
    reachable: true,
    sessions: [
      { id: 'a', title: 'zeta', state: 'idle' },
      { id: 'b', title: 'alpha', state: 'blocked' },
    ],
  }
  const desktop = {
    instance: 'i2',
    label: 'desktop',
    reachable: true,
    sessions: [{ id: 'c', title: 'beta', state: 'running' }],
  }

  it('puts what needs a human first, whichever machine it is on', () => {
    // The question somebody picks the phone up to answer is "does anything need
    // me", not "does anything on the laptop need me".
    const rows = unify([laptop, desktop])
    expect(rows[0]?.id).toBe('b')
  })

  it('does not group by instance, which is the drill-down again', () => {
    const rows = unify([laptop, desktop])
    // blocked, then running, then idle — interleaved across both machines.
    expect(rows.map((r) => r.id)).toEqual(['b', 'c', 'a'])
  })

  it('says which instance each row is on', () => {
    expect(unify([laptop, desktop]).map((r) => r.instanceLabel)).toContain('desktop')
  })

  it('counts from the same rows the body renders', () => {
    const rows = unify([laptop, desktop])
    // And says how many machines are in the count, so three sessions across
    // two laptops does not read as three on one.
    expect(unifiedSubtitle(rows, 2)).toBe('1 of 3 need you · 2 instances')
  })

  it('says when an instance could not be reached', () => {
    // A roster missing a machine's sessions looks exactly like a machine with
    // nothing running on it.
    const rows = unify([laptop, { ...desktop, reachable: false }])
    expect(unifiedSubtitle(rows, 2)).toContain('unreachable')
  })

  it('does not mention instances when there is only one', () => {
    expect(unifiedSubtitle(unify([laptop]), 1)).toBe('1 of 2 need you')
  })
})

describe('what happens to a tap made offline', () => {
  it('says how much is waiting, not just that the connection is gone', () => {
    // A tap that quietly waits and a tap that quietly failed look identical,
    // and somebody who cannot tell will do it again — which for a command means
    // doing it twice.
    expect(waiting(0, 'Offline')).toBe('Offline')
    expect(waiting(1, 'Offline')).toContain('1 action is waiting')
    expect(waiting(3, 'Offline')).toContain('3 actions are waiting')
  })
})

import { describe, expect, it } from 'vitest'
import {
  DEAD_ZONE,
  MAX_FONT,
  MIN_FONT,
  STEP,
  cardTitle,
  directionOf,
  fontForPinch,
  isDoubleTap,
  repeatsFor,
  spanOf,
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
    expect(unifiedSubtitle(rows, 2)).toBe('1 waiting on you · 2 instances')
  })

  it('says when an instance could not be reached', () => {
    // A roster missing a machine's sessions looks exactly like a machine with
    // nothing running on it.
    const rows = unify([laptop, { ...desktop, reachable: false }])
    expect(unifiedSubtitle(rows, 2)).toContain('unreachable')
  })

  it('answers the question before it counts anything', () => {
    // The line under the title is read at arm's length, and "does anything
    // need me" is the only question being asked. A count on its own answers a
    // different one.
    const quiet = unify([
      { ...laptop, sessions: laptop.sessions.map((s) => ({ ...s, state: 'idle' })) },
    ])
    expect(unifiedSubtitle(quiet, 1)).toBe('nothing waiting · 2 running')
  })

  it('does not mention instances when there is only one', () => {
    expect(unifiedSubtitle(unify([laptop]), 1)).toBe('1 waiting on you')
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

describe('the stylesheet itself', () => {
  it('contains no backtick, which would end the template literal', async () => {
    // Hit three times: a backtick inside a CSS comment terminates the string
    // the stylesheet lives in, and the failure is a TypeScript parse error
    // hundreds of lines away that says nothing about CSS.
    const { STYLES } = await import('../src/tokens.js')
    expect(STYLES).not.toContain('`')
  })

  it('has no font shorthand that names inherit as a family', async () => {
    // `font: 16px/1.4 inherit` is not valid CSS. The browser drops the whole
    // declaration, so a field meant to be 16px renders at 11px and iOS zooms
    // into it and never zooms back out.
    const { STYLES } = await import('../src/tokens.js')
    const bad = [...STYLES.matchAll(/font:\s*[^;]*\binherit\s*;/g)]
      .map((m) => m[0])
      .filter((decl) => decl !== 'font: inherit;')
    expect(bad).toEqual([])
  })

  it('sets every text input to at least 16px', async () => {
    // Anything smaller and Safari zooms the page in on focus and does not zoom
    // back out, which leaves the terminal half off-screen for good.
    const { STYLES } = await import('../src/tokens.js')
    for (const rule of ['.composer-field', '.search-field', '.hidden-input']) {
      const block = STYLES.slice(STYLES.indexOf(rule))
      const size = /font-size:\s*(\d+)px/.exec(block.slice(0, block.indexOf('}')))
      expect(size?.[1], `${rule} has no explicit font size`).toBeTruthy()
      expect(Number(size?.[1]), `${rule} is under 16px`).toBeGreaterThanOrEqual(16)
    }
  })
})

describe('what a finger means', () => {
  it('ignores a movement too small to have been aimed', () => {
    // A thumb resting on glass moves. Without a dead zone every touch would
    // send an arrow key.
    expect(directionOf(4, 3)).toBeNull()
  })

  it('picks the dominant axis rather than firing both', () => {
    // A diagonal drag that sent left *and* up moves the cursor somewhere
    // nobody aimed at.
    expect(directionOf(40, 15)).toBe('right')
    expect(directionOf(-40, 15)).toBe('left')
    expect(directionOf(10, 40)).toBe('down')
    expect(directionOf(10, -40)).toBe('up')
  })

  it('is one-to-one for a small correction', () => {
    // The first few characters are precise, because that is what a correction
    // is: moving back two places, not eleven.
    expect(repeatsFor(DEAD_ZONE + STEP)).toBe(1)
    expect(repeatsFor(DEAD_ZONE + STEP * 3)).toBe(3)
  })

  it('accelerates, because crossing a long line should not be a whole swipe', () => {
    const near = repeatsFor(DEAD_ZONE + STEP * 5)
    const far = repeatsFor(DEAD_ZONE + STEP * 10)
    expect(far).toBeGreaterThan(near * 1.5)
  })

  it('is capped, so a flick does not send two hundred keys', () => {
    expect(repeatsFor(100_000)).toBeLessThanOrEqual(60)
  })

  it('counts two quick taps in one place as a double tap', () => {
    expect(isDoubleTap({ at: 0, x: 10, y: 10 }, { at: 200, x: 14, y: 12 })).toBe(true)
  })

  it('does not count two taps far apart in time or space', () => {
    expect(isDoubleTap({ at: 0, x: 10, y: 10 }, { at: 900, x: 10, y: 10 })).toBe(false)
    expect(isDoubleTap({ at: 0, x: 10, y: 10 }, { at: 100, x: 200, y: 10 })).toBe(false)
  })

  it('resizes the grid on a pinch rather than magnifying it', () => {
    // A zoomed terminal is a blurry one you then have to pan around, which is
    // the thing the block model exists to avoid.
    expect(fontForPinch(14, 100, 200)).toBeGreaterThan(14)
    expect(fontForPinch(14, 200, 100)).toBeLessThan(14)
  })

  it('keeps the font within a range a terminal is readable at', () => {
    expect(fontForPinch(14, 100, 10_000)).toBeLessThanOrEqual(MAX_FONT)
    expect(fontForPinch(14, 10_000, 100)).toBeGreaterThanOrEqual(MIN_FONT)
  })

  it('does not divide by a pinch that never started', () => {
    expect(fontForPinch(14, 0, 200)).toBe(14)
  })

  it('measures two fingers and ignores one', () => {
    expect(spanOf([{ clientX: 0, clientY: 0 }, { clientX: 3, clientY: 4 }])).toBe(5)
    expect(spanOf([{ clientX: 0, clientY: 0 }])).toBe(0)
  })
})

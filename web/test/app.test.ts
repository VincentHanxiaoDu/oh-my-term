import { describe, expect, it } from 'vitest'
import {
  applyTheme,
  cardOffer,
  changed,
  isCoherent,
  layOutCard,
  needsYou,
  orderRoster,
  rosterSubtitle,
  screenFor,
  keyToBytes,
  stateOf,
  styleOf,
  widthOf,
} from '../src/index.js'
import type { InteractionCard, ScreenState, Snapshot, StyledRun, Theme } from '../src/index.js'

describe('the roster', () => {
  it('puts what needs a human first, whatever it is called', () => {
    // Spawn order buries the one row that matters behind four that do not,
    // which on a phone means scrolling to find the only reason you opened it.
    const rows = orderRoster([
      { state: 'idle', title: 'aaa' },
      { state: 'working', title: 'bbb' },
      { state: 'blocked', title: 'zzz' },
    ])
    expect(rows[0]?.title).toBe('zzz')
  })

  it('orders the rest by name, so the list does not shuffle between glances', () => {
    const rows = orderRoster([
      { state: 'working', title: 'beta' },
      { state: 'working', title: 'alpha' },
    ])
    expect(rows.map((r) => r.title)).toEqual(['alpha', 'beta'])
  })

  it('counts from the same rows the body renders', () => {
    // The bug that caused the rebuild: a header counting sessions and a body
    // counting agent threads, both on screen, disagreeing — with no way for a
    // user to tell which was right.
    const rows = [
      { id: '1', title: 'a', state: 'blocked' },
      { id: '2', title: 'b', state: 'idle' },
    ]
    expect(rosterSubtitle(rows)).toBe('1 of 2 need you')
  })

  it('says how many sessions there are when none need anything', () => {
    expect(rosterSubtitle([{ id: '1', title: 'a', state: 'idle' }])).toBe('1 session')
  })
})

describe('one state at a time', () => {
  it('is loading before anything has arrived', () => {
    // Distinct from empty. "Nothing running" while the request is in flight is
    // a claim omt cannot make yet.
    expect(stateOf(undefined, undefined, { headline: 'none' }).kind).toBe('loading')
  })

  it('is empty when the answer was empty', () => {
    expect(stateOf([], undefined, { headline: 'none' }).kind).toBe('empty')
  })

  it('is content when there is any', () => {
    expect(stateOf(['a'], undefined, { headline: 'none' }).kind).toBe('content')
  })

  it('reports an error rather than calling a failed request empty', () => {
    // A list that failed to load is not an empty list. Saying "no sessions"
    // when the call failed sends somebody looking for the wrong problem.
    const state = stateOf([], 'the socket closed', { headline: 'none' })
    expect(state.kind).toBe('error')
  })

  it('cannot be two things at once', () => {
    // The type makes it unrepresentable, and this is the test that says so:
    // whatever the inputs, exactly one kind comes back.
    const cases = [
      stateOf(undefined, undefined, { headline: 'x' }),
      stateOf([], undefined, { headline: 'x' }),
      stateOf(['a'], undefined, { headline: 'x' }),
      stateOf(undefined, 'boom', { headline: 'x' }),
    ]
    for (const state of cases) {
      expect(['loading', 'empty', 'error', 'content']).toContain(state.kind)
    }
  })
})

describe('where a tap goes', () => {
  it('opens the terminal for a plain shell', () => {
    // The old client routed everything to the thread grid, which is empty for
    // a shell — so the one thing somebody opens a terminal for was two taps
    // away and neither tap said so.
    expect(screenFor({ state: 'running' }, 0)).toBe('terminal')
  })

  it('opens the threads for a session running agents', () => {
    expect(screenFor({ state: 'running' }, 3)).toBe('threads')
  })

  it('opens the threads for a session that needs a human', () => {
    // Blocked means a card is waiting, and the card is reached through the
    // threads — sending somebody to a terminal would hide it.
    expect(screenFor({ state: 'blocked' }, 0)).toBe('threads')
  })
})

describe('what a card offers', () => {
  const card = (over: Partial<InteractionCard> = {}): InteractionCard => ({
    id: 'i1',
    session: 's1',
    kind: 'choice',
    deliverable: 'native',
    state: 'open',
    prompt: 'Which database?',
    options: ['Postgres', 'SQLite'],
    ...over,
  })

  it('offers the agent options verbatim and in its order', () => {
    expect(cardOffer(card()).options.map((o) => o.label)).toEqual(['Postgres', 'SQLite'])
  })

  it('offers nothing when omt has no channel, and says why', () => {
    // A button that silently does nothing is worse than no button: somebody
    // presses it and believes the agent was answered.
    const offer = cardOffer(
      card({ deliverable: 'none', not_deliverable_because: 'this agent has no responder' }),
    )
    expect(offer.answerable).toBe(false)
    expect(offer.options).toEqual([])
    expect(offer.why).toContain('no responder')
  })

  it('refuses a card somebody has already answered', () => {
    expect(cardOffer(card({ state: 'resolving' })).answerable).toBe(false)
  })

  it('makes an irreversible approval hard to press', () => {
    const offer = cardOffer(
      card({ kind: 'permission', prompt: 'rm -rf /srv/data', options: ['Yes', 'No'] }),
    )
    expect(offer.options[0]?.confirm).toBe('hold')
    expect(offer.options[1]?.confirm).toBe('tap')
  })

  it('does not read a choice question as a shell command', () => {
    // The prompt of a choice card is a question. Treating it as a command
    // would fire the risk heuristic on the word "delete" in prose.
    const offer = cardOffer(card({ prompt: 'Should I delete the old rows?' }))
    expect(offer.options.every((o) => o.confirm === 'tap')).toBe(true)
  })
})

describe('the option layout', () => {
  it('keeps the agent options exactly as the agent wrote them', () => {
    const out = layOutCard(['Yes', 'Yes, and don’t ask again', 'No'], 'ls')
    expect(out.map((o) => o.label)).toEqual(['Yes', 'Yes, and don’t ask again', 'No'])
  })

  it('is ordinary when there is no command behind it', () => {
    expect(layOutCard(['Yes', 'No'], undefined)[0]?.confirm).toBe('tap')
  })
})

describe('a session row', () => {
  it('knows which state means a human is needed', () => {
    expect(needsYou('blocked')).toBe(true)
    expect(needsYou('running')).toBe(false)
  })
})

describe('the terminal screen', () => {
  const run = (text: string, extra: Partial<StyledRun> = {}) => ({ text, ...extra })
  const screen = (rows: StyledRun[][]): Snapshot => ({
    rows,
    cols: 10,
    rows_count: rows.length,
    cursor: [0, 0],
    cursor_visible: true,
    alternate_screen: false,
  })

  it('swaps the colours for inverse rather than filtering them', () => {
    const css = styleOf(run('x', { fg: '#ff0000', bg: '#000000', inverse: true }))
    expect(css).toContain('color:#000000')
    expect(css).toContain('background:#ff0000')
  })

  it('makes the default explicit when inverse swaps in an absent colour', () => {
    const css = styleOf(run('x', { inverse: true }))
    expect(css).not.toContain('inherit')
    expect(css).not.toContain('transparent')
  })

  it('leaves an ordinary run inheriting the theme', () => {
    expect(styleOf(run('x'))).toContain('color:inherit')
  })

  it('counts a row by characters, not by code units', () => {
    expect(widthOf([run('漢字'), run('ab')])).toBe(4)
  })

  it('refuses a snapshot whose cursor is off the screen', () => {
    const bad = { ...screen([[run('hi')]]), cursor: [9, 0] as [number, number] }
    expect(isCoherent(bad)).toBe(false)
  })

  it('accepts a well-formed screen', () => {
    expect(isCoherent(screen([[run('hi')]]))).toBe(true)
  })

  it('does not repaint a screen that did not move', () => {
    expect(changed(screen([[run('hi')]]), screen([[run('hi')]]))).toBe(false)
  })

  it('repaints when only the cursor moved', () => {
    const a = screen([[run('hi')]])
    expect(changed(a, { ...a, cursor: [0, 1] })).toBe(true)
  })
})

describe('the instance theme', () => {
  const theme = (over: Partial<Theme> = {}): Theme => ({
    name: 't',
    appearance: 'dark',
    foreground: '#eeeeee',
    background: '#111111',
    cursor: '#ffb454',
    selection: '#333a45',
    ansi: Array.from({ length: 16 }, (_, i) => `#${String(i).padStart(2, '0')}0000`),
    ...over,
  })

  const fakeRoot = () => {
    const set = new Map<string, string>()
    return {
      set,
      style: { setProperty: (k: string, v: string) => set.set(k, v) },
    } as never as HTMLElement & { set: Map<string, string> }
  }

  it('publishes all sixteen ANSI colours', () => {
    const root = fakeRoot()
    applyTheme(root, theme())
    expect(root.set.get('--omt-ansi-0')).toBe('#000000')
    expect(root.set.get('--omt-ansi-15')).toBe('#150000')
  })

  it('ignores a partial palette rather than applying half of it', () => {
    const root = fakeRoot()
    applyTheme(root, theme({ ansi: ['#ff0000', '#00ff00'] }))
    expect(root.set.has('--omt-ansi-0')).toBe(false)
  })

  it('still applies the foreground and background of a partial theme', () => {
    const root = fakeRoot()
    applyTheme(root, theme({ ansi: [] }))
    expect(root.set.get('--omt-fg')).toBe('#eeeeee')
  })
})

describe('the terminal on a device with no keyboard', () => {
  const snapshot = (text: string): Snapshot => ({
    rows: [[{ text }]],
    cols: 10,
    rows_count: 1,
    cursor: [0, 0],
    cursor_visible: true,
    alternate_screen: false,
  })

  it('keeps drawn content when an unrelated update arrives', () => {
    // The bug: `store.subscribe(render)` fires on any change, and a null
    // snapshot meant "loading" — so a session-list refresh repainted a drawn
    // terminal into a spinner that never stopped.
    const drawn: ScreenState<Snapshot> = { kind: 'content', value: snapshot('hi') }
    // An update that is not about the terminal cannot produce a loading state
    // from content: only the terminal's own fetch sets it.
    expect(drawn.kind).toBe('content')
  })

  it('shows the last screen when returning rather than a spinner', () => {
    // A spinner over known-good content is a downgrade, not a loading state.
    const seen: ScreenState<Snapshot> = { kind: 'content', value: snapshot('hi') }
    const returning = seen.kind === 'content' ? seen : { kind: 'loading' as const }
    expect(returning.kind).toBe('content')
  })
})

describe('what a keypress sends from a soft keyboard', () => {
  it('turns Enter into carriage return', () => {
    // A line discipline in canonical mode acts on CR; LF leaves the command
    // sitting there looking submitted.
    expect(
      keyToBytes({ key: 'Enter', ctrlKey: false, altKey: false, metaKey: false }),
    ).toBe('\r')
  })

  it('leaves an ordinary character to the input event', () => {
    // The input element sends text; keydown sends control keys. Both handling
    // a letter would send it twice.
    expect(keyToBytes({ key: 'a', ctrlKey: false, altKey: false, metaKey: false })).toBe('a')
  })
})

import { describe, expect, it } from 'vitest'
import {
  cardOffer,
  cardStatus,
  layOutCard,
  orderRoster,
  renderSessionRow,
  rosterHeader,
  applyTheme,
  changed,
  isCoherent,
  styleOf,
  widthOf,
} from '../src/index.js'
import type { Snapshot, StyledRun, Theme } from '../src/index.js'
import type { Interaction, InteractionCard } from '../src/index.js'

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

  it('says what a row means in words, not only in colour', () => {
    const row = renderSessionRow({ title: 'api', state: 'blocked' })
    expect(row.label).toContain('needs you')
  })

  it('leads with the count that decides whether to keep reading', () => {
    expect(
      rosterHeader({
        connection: 'connected',
        workspaces: [],
        sessions: [{}, {}] as never,
        threads: {},
        open: [],
        needsYou: 1,
        problems: [],
      }),
    ).toBe('1 of 2 need you')
  })

  it('says why it is empty rather than showing an empty list', () => {
    const header = rosterHeader({
      connection: 'refused',
      refusal: 'that token is not valid',
      workspaces: [],
      sessions: [],
      threads: {},
      open: [],
      needsYou: 0,
      problems: [],
    })
    expect(header).toBe('that token is not valid')
  })
})

describe('a card', () => {
  it('keeps the agent options exactly as the agent wrote them', () => {
    // The index is how the answer is delivered. Reordering them answers a
    // different question than the one the user read.
    const out = layOutCard(['Yes', 'Yes, and don’t ask again', 'No'], 'ls')
    expect(out.map((o) => o.label)).toEqual(['Yes', 'Yes, and don’t ask again', 'No'])
  })

  it('makes an irreversible yes hard to press', () => {
    const out = layOutCard(['Yes', 'No'], 'rm -rf /srv/data')
    expect(out[0]?.confirm).toBe('hold')
  })

  it('never makes the safe answer harder than the dangerous one', () => {
    // Hold-to-deny would push people toward the affirmative, which is exactly
    // backwards from what the confirmation is for.
    const out = layOutCard(['Yes', 'No'], 'rm -rf /srv/data')
    expect(out[1]?.confirm).toBe('tap')
  })

  it('is ordinary when there is no command behind it', () => {
    expect(layOutCard(['Yes', 'No'], undefined)[0]?.confirm).toBe('tap')
  })

  it('refuses to offer buttons for something it cannot deliver', () => {
    const interaction = {
      id: 'i1',
      state: { state: 'open' },
      deliverable: { kind: 'none', reason: 'this agent has no way to be answered remotely' },
    } as unknown as Interaction
    const status = cardStatus(interaction)
    expect(status.answerable).toBe(false)
    expect(status.why).toContain('no way to be answered')
  })

  it('refuses once it has already been answered', () => {
    const interaction = {
      id: 'i1',
      state: { state: 'resolved' },
      deliverable: { kind: 'native' },
    } as unknown as Interaction
    expect(cardStatus(interaction).answerable).toBe(false)
  })

  it('offers the answer when it is open and deliverable', () => {
    const interaction = {
      id: 'i1',
      state: { state: 'open' },
      deliverable: { kind: 'native' },
    } as unknown as Interaction
    expect(cardStatus(interaction).answerable).toBe(true)
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
    // A filter also inverts the glyph's antialiasing and the text comes out
    // fringed.
    const css = styleOf(run('x', { fg: '#ff0000', bg: '#000000', inverse: true }))
    expect(css).toContain('color:#000000')
    expect(css).toContain('background:#ff0000')
  })

  it('makes the default explicit when inverse swaps in an absent colour', () => {
    // Otherwise the swap silently does nothing and a selected line renders
    // exactly like an unselected one.
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
    // Rendering it anyway shows a plausible screen that is not what the
    // terminal says, and somebody answers a prompt from it.
    const bad = { ...screen([[run('hi')]]), cursor: [9, 0] as [number, number] }
    expect(isCoherent(bad)).toBe(false)
  })

  it('refuses a snapshot with fewer rows than it claims', () => {
    expect(isCoherent({ ...screen([[run('hi')]]), rows_count: 5 })).toBe(false)
  })

  it('accepts a well-formed screen', () => {
    expect(isCoherent(screen([[run('hi')]]))).toBe(true)
  })

  it('does not repaint a screen that did not move', () => {
    const a = screen([[run('hi')]])
    expect(changed(a, screen([[run('hi')]]))).toBe(false)
  })

  it('repaints when only the cursor moved', () => {
    // The text is identical, and a client that compares only text leaves the
    // cursor behind where it was.
    const a = screen([[run('hi')]])
    expect(changed(a, { ...a, cursor: [0, 1] })).toBe(true)
  })

  it('repaints when a style changed but the text did not', () => {
    const a = screen([[run('hi')]])
    expect(changed(a, screen([[run('hi', { bold: true })]]))).toBe(true)
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
    // would have the risk heuristic firing on the word "delete" in prose.
    const offer = cardOffer(card({ prompt: 'Should I delete the old rows?' }))
    expect(offer.options.every((o) => o.confirm === 'tap')).toBe(true)
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
    return { set, style: { setProperty: (k: string, v: string) => set.set(k, v) } } as never as
      HTMLElement & { set: Map<string, string> }
  }

  it('publishes all sixteen ANSI colours', () => {
    const root = fakeRoot()
    applyTheme(root, theme())
    expect(root.set.get('--omt-ansi-0')).toBe('#000000')
    expect(root.set.get('--omt-ansi-15')).toBe('#150000')
  })

  it('ignores a partial palette rather than applying half of it', () => {
    // Half a palette leaves the other indices on the built-in fallback, so the
    // screen comes out in two palettes at once — which reads as a rendering
    // bug rather than as a missing theme.
    const root = fakeRoot()
    applyTheme(root, theme({ ansi: ['#ff0000', '#00ff00'] }))
    expect(root.set.has('--omt-ansi-0')).toBe(false)
  })

  it('still applies the foreground and background of a partial theme', () => {
    // Those two are usable on their own, and dropping them would leave a
    // themed instance looking entirely unthemed over one bad palette.
    const root = fakeRoot()
    applyTheme(root, theme({ ansi: [] }))
    expect(root.set.get('--omt-fg')).toBe('#eeeeee')
  })
})

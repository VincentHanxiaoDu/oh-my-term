import { describe, expect, it } from 'vitest'
import { type Thread, actionsFor, cellFor, grid, summarize, toneOf } from '../src/index.js'

function thread(id: string, state: Thread['state'], extra: Partial<Thread> = {}): Thread {
  return { id, is_subagent: true, state, open_interactions: [], ...extra }
}

describe('the subagent grid', () => {
  it('shows five parallel subagents as five cells', () => {
    // The thing Claude Code's own mobile client does not do: with five agents
    // running it shows one session at a time.
    const threads = Array.from({ length: 5 }, (_, i) =>
      thread(`sub${i}`, { state: 'working' }),
    )
    expect(grid(threads)).toHaveLength(5)
  })

  it('puts what needs you first, not what started first', () => {
    // Spawn order buries the one cell that matters behind four that do not,
    // which on a phone means scrolling to find it.
    const threads = [
      thread('a', { state: 'working' }),
      thread('b', { state: 'idle' }),
      thread('c', { state: 'blocked', reason: 'permission', interaction: 'i1' }),
      thread('d', { state: 'exited' }),
    ]
    expect(grid(threads).map((c) => c.thread.id)).toEqual(['c', 'a', 'b', 'd'])
  })

  it('is readable without colour', () => {
    // A grid whose only signal is hue does not tell some users which agent is
    // stuck.
    const glyphs = new Set(
      (['blocked', 'working', 'idle', 'exited', 'unknown'] as const).map(
        (s) => cellFor(thread('x', { state: s } as Thread['state'])).glyph,
      ),
    )
    expect(glyphs.size).toBe(5)
  })

  it('gives every cell something a screen reader can say', () => {
    const c = cellFor(
      thread('sub1', { state: 'blocked', reason: 'permission', interaction: 'i1' }, {
        label: 'review the auth module',
      }),
    )
    expect(c.label).toBe('review the auth module — needs you')
  })

  it('marks only a blocked cell as actionable now', () => {
    // Tapping a working agent is fine, but it must not look like it will
    // change anything.
    expect(cellFor(thread('a', { state: 'blocked', reason: 'input' })).actionable).toBe(true)
    expect(cellFor(thread('b', { state: 'working' })).actionable).toBe(false)
  })

  it('leads with the number worth leading with', () => {
    const threads = [
      thread('a', { state: 'working' }),
      thread('b', { state: 'blocked', reason: 'permission', interaction: 'i1' }),
      thread('c', { state: 'working' }),
    ]
    expect(summarize(threads)).toBe('1 of 3 need you')
    expect(summarize([thread('a', { state: 'working' })])).toBe('1 of 1 working')
    expect(summarize([])).toBe('nothing running')
  })

  it('never offers to prompt a running subagent', () => {
    // There is no input channel to deliver one into. Rendering a text box omt
    // cannot reach is the same mistake as an answer button for an
    // undeliverable card.
    const actions = actionsFor(thread('sub1', { state: 'working' }))
    expect(actions.every((a) => a.action !== ('prompt' as never))).toBe(true)
    expect(actions).toEqual([{ action: 'open-transcript', thread: 'sub1' }])
  })

  it('offers to answer each card a subagent raised', () => {
    // Five subagents blocked on five questions is five answers to give.
    const t = thread('sub1', { state: 'blocked', reason: 'permission', interaction: 'i1' }, {
      open_interactions: ['i1', 'i2'],
    })
    expect(actionsFor(t).filter((a) => a.action === 'answer')).toHaveLength(2)
  })

  it('draws the same order twice', () => {
    // Cells that reshuffle between renders are cells the user mistaps.
    const threads = [
      thread('b', { state: 'working' }),
      thread('a', { state: 'working' }),
      thread('c', { state: 'working' }),
    ]
    expect(grid(threads).map((c) => c.thread.id)).toEqual(
      grid(threads).map((c) => c.thread.id),
    )
  })

  it('maps every agent state to a tone', () => {
    expect(toneOf({ state: 'blocked', reason: 'input' })).toBe('blocked')
    expect(toneOf({ state: 'starting' })).toBe('unknown')
    expect(toneOf({ state: 'exited' })).toBe('finished')
  })
})

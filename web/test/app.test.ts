import { describe, expect, it } from 'vitest'
import {
  cardStatus,
  layOutCard,
  orderRoster,
  renderSessionRow,
  rosterHeader,
} from '../src/index.js'
import type { Interaction } from '../src/index.js'

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

import { describe, expect, it } from 'vitest'

import {
  BUDGET,
  STORES,
  TRIM_ORDER,
  canFitWithinBudget,
  isEvictable,
  mayHoldTerminalContent,
  planTrim,
  total,
  type Usage,
} from '../src/cache'

const comfortable: Usage = {
  identity: 4 * 1024,
  keys: 1024,
  credentials: 8 * 1024,
  prefs: 16 * 1024,
  terminal: 1024 * 1024,
  sessions: 128 * 1024,
  interactions: 64 * 1024,
  instances: 64 * 1024,
}

describe('the storage budget', () => {
  it('asks for nothing when there is room', () => {
    expect(planTrim(comfortable)).toEqual([])
  })

  it('trims the largest and cheapest thing first', () => {
    // `terminal` is a cache of something the instance still has, and
    // refetching it costs one round trip.
    const full: Usage = { ...comfortable, terminal: 8 * 1024 * 1024 }
    const plan = planTrim(full)
    expect(plan[0]?.store).toBe('terminal')
  })

  it('never proposes dropping something that would un-pair the device', () => {
    // A plan that suggested the identity record would un-pair the device to
    // save eight kilobytes, and the person is then holding a phone that has
    // forgotten which machine it belongs to.
    const full: Usage = { ...comfortable, terminal: 32 * 1024 * 1024 }
    const named = planTrim(full).map((t) => t.store)
    expect(named).not.toContain('identity')
    expect(named).not.toContain('keys')
    expect(named).not.toContain('credentials')
  })

  it('moves on to the next store when the first is not enough', () => {
    // The overage has to exceed what `terminal` holds, or trimming it alone
    // covers the whole thing — which is what my first version of this test
    // actually described while claiming otherwise.
    const full: Usage = {
      ...comfortable,
      terminal: 1 * 1024 * 1024,
      sessions: 12 * 1024 * 1024,
    }
    const plan = planTrim(full)
    expect(plan.map((t) => t.store)).toEqual(['terminal', 'sessions'])
    expect(plan[0]?.bytes).toBe(1 * 1024 * 1024)
  })

  it('trims exactly what is over and no more', () => {
    // Trimming to zero because it was easier would throw away a cache that was
    // mostly fine, and the next three sessions somebody opens are slow for it.
    const full: Usage = { ...comfortable, terminal: BUDGET }
    const trimmed = planTrim(full).reduce((sum, t) => sum + t.bytes, 0)
    expect(trimmed).toBe(total(full) - BUDGET)
    expect(trimmed).toBeGreaterThan(0)
    expect(trimmed).toBeLessThan(full.terminal ?? 0)
  })

  it('follows the documented order', () => {
    expect(TRIM_ORDER).toEqual(['terminal', 'sessions', 'interactions', 'instances'])
  })

  it('says when trimming cannot help rather than looping', () => {
    // A loop that trims everything it may, is still over, and trims again on
    // the next write burns the device's battery to no effect.
    const impossible: Usage = { identity: BUDGET * 2 }
    expect(canFitWithinBudget(impossible)).toBe(false)
    expect(canFitWithinBudget(comfortable)).toBe(true)
  })
})

describe('what each store may hold', () => {
  it('only the terminal store holds terminal content', () => {
    // A cached session row is a thing a colleague may see over somebody's
    // shoulder on a lock screen, and a title is a very different exposure from
    // the last thing the shell printed.
    expect(mayHoldTerminalContent('terminal')).toBe(true)
    for (const name of ['sessions', 'interactions', 'instances', 'prefs']) {
      expect(mayHoldTerminalContent(name)).toBe(false)
    }
  })

  it('the stores that must survive are marked so', () => {
    for (const name of ['identity', 'keys', 'credentials', 'prefs']) {
      expect(isEvictable(name)).toBe(false)
    }
    for (const name of TRIM_ORDER) {
      expect(isEvictable(name)).toBe(true)
    }
  })

  it('every store in the trim order exists', () => {
    // An order naming a store nothing writes would silently trim nothing and
    // move on, which reads as the budget not working.
    for (const name of TRIM_ORDER) {
      expect(STORES.some((s) => s.name === name)).toBe(true)
    }
  })

  it('every store has a cap or a reason not to', () => {
    for (const store of STORES) {
      if (store.cap === 0) {
        // Only the key store, whose size is not ours to choose.
        expect(store.name).toBe('keys')
      } else {
        expect(store.cap).toBeGreaterThan(0)
      }
    }
  })

  it('the caps add up to less than the budget', () => {
    // Otherwise every store can be individually within its cap and the whole
    // still over, and the trim runs on a device that did nothing wrong.
    const sum = STORES.reduce((total, s) => total + s.cap, 0)
    expect(sum).toBeLessThanOrEqual(BUDGET)
  })
})

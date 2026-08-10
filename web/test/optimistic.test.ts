import { describe, expect, it } from 'vitest'

import { CAPABILITY_INFO, CAPABILITIES } from '../src/generated/catalog'
import { mayBeOptimistic, optimisticRefusal, pendingLabel } from '../src/optimistic'

describe('what may be shown before the server agrees', () => {
  it('lets purely local and recoverable things through', () => {
    // Applying every change only after confirmation makes the UI feel broken
    // over a 250ms round trip.
    expect(mayBeOptimistic('session.list')).toBe(true)
    expect(mayBeOptimistic('queue.enqueue')).toBe(true)
  })

  it('refuses anything the catalog calls destructive', () => {
    // Mechanical rather than a maintained list: a hand-kept one is wrong the
    // day a destructive capability is added, and wrong in the direction that
    // shows somebody a deletion that did not happen.
    expect(mayBeOptimistic('session.close')).toBe(false)
    expect(optimisticRefusal('session.close')).toEqual({ reason: 'destructive' })
  })

  it('refuses every destructive capability in the catalog, not a sample', () => {
    const destructive = CAPABILITIES.filter((name) =>
      CAPABILITY_INFO[name].effects.includes('destructive'),
    )
    expect(destructive.length).toBeGreaterThan(0)
    for (const name of destructive) {
      expect(mayBeOptimistic(name), name).toBe(false)
    }
  })

  it('refuses resolving an interaction, whatever its effects say', () => {
    // The failure mode is "you think you denied something you approved". It
    // declares no effect bits, so the effect rule alone would have let it
    // through.
    expect(mayBeOptimistic('interaction.resolve')).toBe(false)
    expect(optimisticRefusal('interaction.resolve')?.reason).toBe('exactly-once')
  })

  it('offers the honest intermediate wording for an interaction', () => {
    // `Submitted` is a real state -- sent, not yet confirmed by the agent --
    // and rendering `Resolved` there would be the optimism this forbids.
    expect(pendingLabel('interaction.resolve')).toBe('sent — waiting for the agent to confirm')
  })

  it('refuses a compare-and-swap setting', () => {
    // Showing the new value before it lands invites a lost update: two clients
    // each see their own value and one of them is wrong.
    expect(mayBeOptimistic('config.import')).toBe(false)
  })

  it('refuses a takeover that has a grace window', () => {
    // The window is the point of it; there is nothing to be optimistic about
    // while it runs.
    expect(mayBeOptimistic('session.acquire')).toBe(false)
  })

  it('is conservative about a capability this build does not know', () => {
    // It may belong to a newer instance, and guessing on behalf of something
    // we cannot describe is exactly the case to be careful in.
    expect(mayBeOptimistic('future.capability')).toBe(false)
  })

  it('says what is being waited for rather than showing a bare spinner', () => {
    // A spinner with no words is indistinguishable from a hang.
    expect(pendingLabel('session.close')).toBeTruthy()
    expect(pendingLabel('session.list')).toBeNull()
  })
})

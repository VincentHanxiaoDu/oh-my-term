import { describe, expect, it } from 'vitest'

import { CAPABILITIES } from '../src/generated/catalog'
import { handledCapabilities } from '../src/capabilities'
import { unavailable, unreachable } from '../src/parity'

describe('the parity gate', () => {
  it('handles every capability this build declares', () => {
    // The type-level form of this is in `parity.ts` and fails at compile time,
    // naming the offender. This is the same question at runtime, so a failure
    // reads as a list rather than as a type error somebody has to decode.
    const handled = new Set(handledCapabilities())
    const missing = CAPABILITIES.filter((name) => !handled.has(name))
    expect(missing).toEqual([])
  })

  it('is checking a real set rather than an empty one', () => {
    // A gate over nothing reports success without checking anything.
    expect(CAPABILITIES.length).toBeGreaterThan(100)
    expect(handledCapabilities().length).toBeGreaterThan(100)
  })

  it('reports what a newer instance offers that this client cannot call', () => {
    // A phone attached to several instances meets capabilities its own catalog
    // never had. 03 §7 says it renders the intersection rather than failing, so
    // this reports rather than throws.
    expect(unreachable([...CAPABILITIES, 'future.capability'])).toEqual([
      'future.capability',
    ])
  })

  it('reports what this client can call that an older instance does not offer', () => {
    // The greyed-out half of the intersection: the button is hidden with a
    // reason rather than left to fail when tapped.
    const withoutOne = CAPABILITIES.filter((name) => name !== 'session.list')
    expect(unavailable(withoutOne)).toContain('session.list')
  })

  it('says nothing when the two agree', () => {
    expect(unreachable(CAPABILITIES)).toEqual([])
    expect(unavailable(CAPABILITIES)).toEqual([])
  })
})

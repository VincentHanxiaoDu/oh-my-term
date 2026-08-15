import { describe, expect, it } from 'vitest'
import { attention, stateWord } from '../src/federation.js'
import vocabulary from '../src/generated/vocabulary.json' with { type: 'json' }

/**
 * The words this client uses must be the words the terminal uses.
 *
 * They drifted once: the terminal said "waiting on you" and this said
 * "blocked", about the same session, in the same product — and a person found
 * it, not a test. One implementation cannot be shared across a Rust binary and
 * a browser, so what is shared is the table, generated from the Rust source by
 * `cargo xtask vocabulary`. This is the half that fails when they disagree.
 *
 * The same shape as the capability parity gate, and for the same reason: two
 * lists that have to agree will not, unless one of them is generated.
 */
describe('both surfaces speak one language', () => {
  /** How the generated table names a state, given the wire word and `seen`. */
  const keyFor = (state: string, seen: boolean): string => {
    if (state === 'idle') {
      return seen ? 'Idle' : 'Idle if !seen'
    }
    return state.charAt(0).toUpperCase() + state.slice(1)
  }

  it('uses the terminal word for every state the terminal knows', () => {
    for (const [key, word] of Object.entries(vocabulary.long)) {
      // The wire spelling of the state this key names.
      const [wire, seen] =
        key === 'Idle if !seen'
          ? (['idle', false] as const)
          : ([key.toLowerCase(), true] as const)
      expect(stateWord(wire, seen), `${key} → ${word}`).toBe(word)
    }
  })

  it('orders by the same ranks', () => {
    for (const [key, rank] of Object.entries(vocabulary.attention)) {
      const [wire, seen] =
        key === 'Idle if !seen'
          ? (['idle', false] as const)
          : ([key.toLowerCase(), true] as const)
      expect(attention(wire, seen), `${key} → ${rank}`).toBe(Number(rank))
    }
  })

  it('never says unknown, in either language', () => {
    for (const table of [vocabulary.long, vocabulary.short]) {
      for (const word of Object.values(table)) {
        expect(word).not.toContain('unknown')
      }
    }
  })

  it('has a short form for every long one', () => {
    // The panel is twenty-three columns and takes the short form. A state with
    // a long word and no short one is a row that cuts mid-sentence.
    expect(Object.keys(vocabulary.short).sort()).toEqual(Object.keys(vocabulary.long).sort())
  })
})

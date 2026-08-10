/**
 * What the UI may show before the server has agreed.
 *
 * `docs/architecture/12-collaboration.md` §7. A phone over a relayed tailnet
 * has a 100–250 ms round trip. Applying every change only after confirmation
 * makes the UI feel broken; applying changes optimistically and getting them
 * wrong makes it *be* broken. This is the line between the two.
 *
 * The rule is **mechanical and catalog-driven**, not a list somebody maintains:
 * anything declaring `destructive` may not be applied optimistically, and the
 * declaration comes from the same generated catalog the server registers. A
 * hand-kept list would be wrong the day a destructive capability is added, and
 * wrong in the direction that shows somebody a deletion that did not happen.
 */

import { CAPABILITY_INFO, type Capability } from './generated/catalog'

/** Why a change may not be shown before it is confirmed. */
export type Refusal =
  /** It declares `destructive`. */
  | { reason: 'destructive' }
  /** Exactly-once and consequential. */
  | { reason: 'exactly-once'; detail: string }
  /** Compare-and-swap; showing the new value invites a lost update. */
  | { reason: 'cas'; detail: string }
  /** It has a grace window, so there is nothing to be optimistic about. */
  | { reason: 'has-a-grace-window'; detail: string }

/**
 * Capabilities that may not be optimistic for a reason the effect bits do not
 * carry.
 *
 * Short and each entry says why, because every one of these is a judgement
 * rather than a mechanical fact — and a judgement with no reason attached is
 * one the next person deletes.
 */
const NEVER: Partial<Record<Capability, Refusal>> = {
  // The failure mode is "you think you denied something you approved". Doubly
  // so on the synthetic path, where the honest intermediate rendering is
  // `Submitted` — "sent, waiting for the agent to confirm" — which is a real
  // state and not an optimistic one.
  'interaction.resolve': {
    reason: 'exactly-once',
    detail: 'sent — waiting for the agent to confirm',
  },
  // Showing the new configuration before it lands invites a lost update: two
  // clients each see their own value and one of them is wrong. (12 §7 names
  // `config.set`; this build's write path is `config.import`.)
  'config.import': {
    reason: 'cas',
    detail: 'the settings are compared against what is stored',
  },
  // A five-second grace window is the point of it; there is nothing to be
  // optimistic about while it runs.
  'session.acquire': {
    reason: 'has-a-grace-window',
    detail: 'the current writer has five seconds to object',
  },
}

/**
 * Whether the UI may apply this change before the server confirms it.
 *
 * Returns `null` when it may, and the reason when it may not — so a caller
 * cannot accidentally treat "not allowed" as a boolean it forgot to check.
 */
export function optimisticRefusal(capability: string): Refusal | null {
  const named = NEVER[capability as Capability]
  if (named) return named

  const info = CAPABILITY_INFO[capability as Capability]
  // A capability this build does not know is not one to be optimistic about.
  // It may be a newer instance's, and guessing on behalf of something we
  // cannot describe is exactly the case to be conservative in.
  if (!info) return { reason: 'destructive' }

  if (info.effects.includes('destructive')) return { reason: 'destructive' }
  return null
}

/** Whether the UI may apply this change before the server confirms it. */
export function mayBeOptimistic(capability: string): boolean {
  return optimisticRefusal(capability) === null
}

/**
 * What to render for an action that cannot be optimistic.
 *
 * A spinner with no words is indistinguishable from a hang; these say what is
 * being waited for.
 */
export function pendingLabel(capability: string): string | null {
  const refusal = optimisticRefusal(capability)
  if (!refusal) return null
  switch (refusal.reason) {
    case 'destructive':
      return 'waiting for confirmation'
    default:
      return refusal.detail
  }
}

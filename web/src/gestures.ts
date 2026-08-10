/**
 * What a finger means.
 *
 * `docs/architecture/24-mobile.md` §3.2. A phone has no arrow keys, no Tab and
 * no scroll wheel, and reaching the key bar for every cursor movement is what
 * makes a mobile terminal feel like a toy. These are the gestures that replace
 * them.
 *
 * The decisions live here, apart from the DOM, because they are the part that
 * can be wrong: how far a drag has to go before it means a key, how fast it
 * accelerates, what counts as a double tap. Attaching a listener cannot be
 * wrong in an interesting way.
 *
 * Nothing destructive is on a gesture. A movement somebody makes by accident
 * must not be able to stop an agent.
 */

/** How far a finger must travel before it means anything, in CSS pixels. */
export const DEAD_ZONE = 12

/** How far one arrow key is worth at the start of a drag. */
export const STEP = 28

/**
 * How long a press must last to become a drag-for-arrows rather than a tap.
 *
 * Long enough that scrolling the screen does not arm it, short enough that
 * nobody wonders whether it is working. Named apart from the hold that confirms
 * an irreversible answer: they are different gestures with different stakes,
 * and one constant serving both would tie them together for no reason.
 */
export const DRAG_ARM_MS = 320

/** How close together two taps must be to count as one gesture. */
export const DOUBLE_TAP_MS = 300

/** How far apart two taps may be and still be the same spot. */
export const DOUBLE_TAP_SLOP = 24

/** Which way a drag has gone, once it has gone far enough to mean anything. */
export type Direction = 'up' | 'down' | 'left' | 'right' | null

/**
 * Which arrow a drag is asking for.
 *
 * The dominant axis wins outright rather than both firing: a diagonal drag that
 * sent both a left and an up would move the cursor somewhere nobody aimed at.
 */
export function directionOf(dx: number, dy: number): Direction {
  if (Math.abs(dx) < DEAD_ZONE && Math.abs(dy) < DEAD_ZONE) {
    return null
  }
  if (Math.abs(dx) >= Math.abs(dy)) {
    return dx > 0 ? 'right' : 'left'
  }
  return dy > 0 ? 'down' : 'up'
}

/** The bytes an arrow sends. */
export const ARROWS: Record<Exclude<Direction, null>, string> = {
  up: '[A',
  down: '[B',
  left: '[D',
  right: '[C',
}

/**
 * How many keys a drag of this distance is worth.
 *
 * Accelerating, because a linear drag makes crossing a long line a whole-screen
 * swipe. The first few characters are one-to-one so a small correction is
 * precise, and it opens up after that.
 */
export function repeatsFor(distance: number): number {
  const past = Math.max(0, Math.abs(distance) - DEAD_ZONE)
  const linear = Math.floor(past / STEP)
  if (linear <= 4) {
    return linear
  }
  // Beyond four, each step is worth progressively more — capped, because a
  // flick across the screen should not send two hundred keys.
  return Math.min(60, 4 + Math.floor((linear - 4) * 1.8))
}

/** Whether two taps are one double tap. */
export function isDoubleTap(
  first: { at: number; x: number; y: number },
  second: { at: number; x: number; y: number },
): boolean {
  return (
    second.at - first.at <= DOUBLE_TAP_MS &&
    Math.hypot(second.x - first.x, second.y - first.y) <= DOUBLE_TAP_SLOP
  )
}

/** The smallest and largest the terminal font may be pinched to. */
export const MIN_FONT = 9
/** Likewise. */
export const MAX_FONT = 28

/**
 * The font size a pinch is asking for.
 *
 * A pinch resizes the *grid* — the font, and therefore how many columns fit,
 * and therefore a `SIGWINCH`. It is deliberately not a bitmap zoom: zooming a
 * terminal produces a blurry grid you then have to pan around, which is the
 * thing the block model exists to avoid.
 */
export function fontForPinch(startFont: number, startSpan: number, nowSpan: number): number {
  if (startSpan <= 0) {
    return startFont
  }
  const wanted = startFont * (nowSpan / startSpan)
  return Math.round(Math.min(MAX_FONT, Math.max(MIN_FONT, wanted)))
}

/** How far two fingers are apart. */
export function spanOf(touches: { clientX: number; clientY: number }[]): number {
  const [a, b] = touches
  if (a === undefined || b === undefined) {
    return 0
  }
  return Math.hypot(b.clientX - a.clientX, b.clientY - a.clientY)
}

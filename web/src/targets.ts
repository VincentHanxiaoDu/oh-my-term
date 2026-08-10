/**
 * Making a recognised span tappable on a phone.
 *
 * `docs/architecture/18-semantic-open.md` §7. In a browser the mouse conflict
 * evaporates — the web client renders the grid itself and no inner program has
 * grabbed anything — so this surface gets the best version of the feature
 * rather than a degraded one.
 *
 * The constraint that shapes all of it: a 44 px minimum touch target against a
 * terminal cell about 8 px wide. Tapping a match in a rendered grid is not
 * viable and is not attempted. Instead every match gets an expanded hit area,
 * and where two of those overlap the tap opens a disambiguation popover rather
 * than guessing.
 *
 * Guessing is the thing being avoided. A tap that lands between two links and
 * silently picks one is indistinguishable from a tap that worked, right up
 * until it opens the wrong file — and the user has no way to know it happened.
 */

/** The smallest touch target that is reliably hittable, in CSS pixels. */
export const MIN_TOUCH = 44

/** One recognised span, in grid coordinates. */
export interface Span {
  /** Byte offset of the first character within the line. */
  start: number
  /** One past the last. */
  end: number
  /** The text of it, shown when disambiguating. */
  text: string
  /** Which row of the grid it is on. */
  row: number
}

/** A hit area in CSS pixels. */
export interface HitArea {
  span: Span
  left: number
  top: number
  width: number
  height: number
}

/** The geometry of the rendered grid. */
export interface Cell {
  width: number
  height: number
}

/**
 * Expand a span into something a finger can hit.
 *
 * Height is padded to the minimum and centred on the row, so the target grows
 * equally into the rows above and below rather than covering the line beneath —
 * a target that grew downward only would sit on top of the next line's matches
 * and steal every tap meant for them.
 *
 * Width is *not* padded to the minimum. Clamping horizontally to the span is
 * deliberate: a padded-out narrow match would overlap its neighbours on the
 * same line, and on a terminal line that is exactly where several matches sit
 * next to each other. Narrow matches are reached through the disambiguation
 * popover instead, which is honest about there being a choice.
 */
export function hitArea(span: Span, cell: Cell): HitArea {
  const width = (span.end - span.start) * cell.width
  const height = Math.max(MIN_TOUCH, cell.height)
  const overflow = (height - cell.height) / 2
  return {
    span,
    left: span.start * cell.width,
    top: span.row * cell.height - overflow,
    width,
    height,
  }
}

/** Whether two hit areas overlap enough that a tap is ambiguous. */
export function overlaps(a: HitArea, b: HitArea): boolean {
  const horizontal = a.left < b.left + b.width && b.left < a.left + a.width
  const vertical = a.top < b.top + b.height && b.top < a.top + a.height
  return horizontal && vertical
}

/**
 * Which matches a tap at this point could have meant.
 *
 * Returns every candidate rather than the nearest. One candidate is an
 * activation; more than one is a popover. The caller does not get to skip the
 * popover by taking the first, because the ordering here is grid order and
 * carries no claim about which the user wanted.
 */
export function candidatesAt(
  x: number,
  y: number,
  spans: Span[],
  cell: Cell,
): Span[] {
  return spans
    .map((span) => hitArea(span, cell))
    .filter(
      (area) =>
        x >= area.left &&
        x <= area.left + area.width &&
        y >= area.top &&
        y <= area.top + area.height,
    )
    .map((area) => area.span)
}

/**
 * Group spans into the sets that would need a popover between them.
 *
 * Used to decide, before any tap, which matches can be activated directly. A
 * client that only discovered ambiguity on tap would have to re-render the
 * moment somebody touched the screen.
 */
export function ambiguousGroups(spans: Span[], cell: Cell): Span[][] {
  const areas = spans.map((span) => hitArea(span, cell))
  const groups: Span[][] = []
  const taken = new Set<number>()
  for (let i = 0; i < areas.length; i++) {
    if (taken.has(i)) continue
    const group = [areas[i]!.span]
    taken.add(i)
    for (let j = i + 1; j < areas.length; j++) {
      if (taken.has(j)) continue
      if (areas.some((a, k) => taken.has(k) && overlaps(a, areas[j]!))) {
        group.push(areas[j]!.span)
        taken.add(j)
      }
    }
    groups.push(group)
  }
  return groups.filter((g) => g.length > 1)
}

/**
 * What the popover says about a candidate.
 *
 * The text, not the kind. Somebody choosing between two links on the same line
 * is distinguishing them by what they say — telling them both are paths is
 * true and useless.
 */
export function candidateLabel(span: Span): string {
  const text = span.text.trim()
  if (text.length <= 48) {
    return text
  }
  // Truncated from the middle: the two ends of a path are what distinguish it,
  // and a list of candidates all reading `/very/long/prefix/…` is a list with
  // no information in it.
  return `${text.slice(0, 24)}…${text.slice(-20)}`
}

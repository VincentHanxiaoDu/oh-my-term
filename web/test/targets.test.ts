import { describe, expect, it } from 'vitest'

import {
  MIN_TOUCH,
  ambiguousGroups,
  candidateLabel,
  candidatesAt,
  hitArea,
  overlaps,
  type Cell,
  type Span,
} from '../src/targets'

/** A realistic terminal cell: about 8 px wide against a 44 px touch target. */
const cell: Cell = { width: 8, height: 17 }

function span(start: number, end: number, row = 0, text = 'src/main.rs'): Span {
  return { start, end, row, text }
}

describe('hit areas', () => {
  it('grows a span to something a finger can actually hit', () => {
    // The constraint the whole module exists for: 44 px against an 8 px cell.
    const area = hitArea(span(0, 11), cell)
    expect(area.height).toBeGreaterThanOrEqual(MIN_TOUCH)
  })

  it('grows equally into the rows above and below', () => {
    // A target that grew downward only would sit on top of the next line's
    // matches and steal every tap meant for them.
    const area = hitArea(span(0, 11, 5), cell)
    const rowTop = 5 * cell.height
    const above = rowTop - area.top
    const below = area.top + area.height - (rowTop + cell.height)
    expect(above).toBeCloseTo(below)
  })

  it('does not pad the width out to the minimum', () => {
    // Deliberate. A padded-out narrow match would overlap its neighbours on
    // the same line, which is exactly where several matches sit side by side.
    const area = hitArea(span(0, 2), cell)
    expect(area.width).toBe(16)
    expect(area.width).toBeLessThan(MIN_TOUCH)
  })

  it('places the span where the text is', () => {
    const area = hitArea(span(10, 20, 3), cell)
    expect(area.left).toBe(80)
    expect(area.width).toBe(80)
  })
})

describe('when a tap is ambiguous', () => {
  it('reports every candidate rather than the nearest', () => {
    // A tap that lands between two links and silently picks one is
    // indistinguishable from one that worked, until it opens the wrong file.
    //
    // The ambiguity that actually arises is vertical: padding a 17 px row to
    // 44 px makes every match overlap the ones above and below it, so a tap
    // near a row boundary genuinely could have meant either.
    const spans = [span(0, 4, 0, 'a.rs'), span(0, 4, 1, 'b.rs')]
    const found = candidatesAt(8, 17, spans, cell)
    expect(found).toHaveLength(2)
    expect(found.map((s) => s.text)).toEqual(['a.rs', 'b.rs'])
  })

  it('a tap in the gap between two spans finds neither', () => {
    // Rather than snapping to the closest. Nothing happening is a result
    // somebody can see and retry; the wrong file opening is not.
    const spans = [span(0, 4, 0, 'a.rs'), span(5, 9, 0, 'b.rs')]
    expect(candidatesAt(36, 8, spans, cell)).toEqual([])
  })

  it('finds nothing where there is nothing', () => {
    const found = candidatesAt(500, 500, [span(0, 4)], cell)
    expect(found).toEqual([])
  })

  it('a tap inside one span alone returns exactly that one', () => {
    const spans = [span(0, 4, 0, 'a.rs'), span(40, 44, 0, 'b.rs')]
    expect(candidatesAt(8, 8, spans, cell)).toHaveLength(1)
  })

  it('spans on adjacent rows overlap because the targets were grown', () => {
    // The consequence of padding to 44 px on a 17 px row, and the reason
    // disambiguation exists at all rather than being a rare case.
    const a = hitArea(span(0, 4, 0), cell)
    const b = hitArea(span(0, 4, 1), cell)
    expect(overlaps(a, b)).toBe(true)
  })

  it('spans far apart on the same row do not overlap', () => {
    const a = hitArea(span(0, 4, 0), cell)
    const b = hitArea(span(40, 44, 0), cell)
    expect(overlaps(a, b)).toBe(false)
  })

  it('groups the matches that would need a popover between them', () => {
    // Worked out before any tap. A client that only discovered ambiguity on
    // tap would have to re-render the moment somebody touched the screen.
    const spans = [span(0, 4, 0, 'a.rs'), span(0, 4, 1, 'b.rs')]
    const groups = ambiguousGroups(spans, cell)
    expect(groups).toHaveLength(1)
    expect(groups[0]).toHaveLength(2)
  })

  it('leaves unambiguous matches out of the groups', () => {
    const spans = [span(0, 4, 0, 'a.rs'), span(40, 44, 0, 'b.rs')]
    expect(ambiguousGroups(spans, cell)).toEqual([])
  })
})

describe('what the popover says', () => {
  it('shows the text, because that is what distinguishes two links', () => {
    // Telling somebody both candidates are paths is true and useless.
    expect(candidateLabel(span(0, 11, 0, 'src/main.rs'))).toBe('src/main.rs')
  })

  it('truncates from the middle so the ends survive', () => {
    // The two ends of a path are what distinguish it. A list of candidates all
    // reading `/very/long/prefix/…` has no information in it.
    const long = `/very/long/prefix/${'x'.repeat(60)}/distinctive-name.rs`
    const label = candidateLabel(span(0, long.length, 0, long))
    expect(label.length).toBeLessThan(long.length)
    expect(label).toContain('/very/long/prefix')
    expect(label).toContain('distinctive-name.rs')
  })
})

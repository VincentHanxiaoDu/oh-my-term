import { describe, expect, it } from 'vitest'
import {
  MAX_COLS,
  MIN_COLS,
  MIN_ROWS,
  fit,
  isLineOriented,
  sizeForColumns,
  trimTrailingBlank,
  worthResizing,
} from '../src/index.js'

describe('fitting a grid', () => {
  it('a phone gets the columns it actually has', () => {
    // 390pt wide, ~7.2px per cell at 12px monospace.
    expect(fit({ width: 390, height: 600, cellWidth: 7.2, cellHeight: 16 })).toEqual({
      cols: 54,
      rows: 37,
    })
  })

  it('a zero cell does not divide by zero', () => {
    // This is called before the webfont loads more often than not, and it is
    // the commonest bug in browser terminals.
    expect(fit({ width: 390, height: 600, cellWidth: 0, cellHeight: 0 })).toEqual({
      cols: MIN_COLS,
      rows: MIN_ROWS,
    })
  })

  it('a zero viewport is clamped rather than producing a zero grid', () => {
    // A window manager really does report zero mid-drag, and a zero-column
    // grid makes every downstream width calculation a division by zero.
    const f = fit({ width: 0, height: 0, cellWidth: 7, cellHeight: 16 })
    expect(f.cols).toBeGreaterThanOrEqual(MIN_COLS)
    expect(f.rows).toBeGreaterThanOrEqual(MIN_ROWS)
  })

  it('an absurd viewport is capped', () => {
    const f = fit({ width: 100_000, height: 100_000, cellWidth: 1, cellHeight: 1 })
    expect(f.cols).toBe(MAX_COLS)
  })

  it('only a whole cell of change is worth a resize', () => {
    // A resize is a SIGWINCH and a full redraw on the far side; one per pixel
    // of a drag would redraw the program sixty times a second.
    expect(worthResizing({ cols: 80, rows: 24 }, { cols: 80, rows: 24 })).toBe(false)
    expect(worthResizing({ cols: 80, rows: 24 }, { cols: 81, rows: 24 })).toBe(true)
  })

  it('a target column count maps to a font size', () => {
    // The pinch gesture: the user asks for 80 columns and gets whatever size
    // that takes, rather than scaling a bitmap.
    const size = sizeForColumns(390, 80)
    expect(size).toBeGreaterThan(0)
    expect(sizeForColumns(390, 40)).toBeGreaterThan(size)
  })

  it('a nonsense column count does not produce a nonsense size', () => {
    expect(sizeForColumns(390, 0)).toBeGreaterThan(0)
    expect(sizeForColumns(390, -5)).toBeGreaterThan(0)
  })

  it('a full-screen program is not rendered as lines', () => {
    // Rendering vim as a transcript looks like omt is broken rather than like
    // vim is running.
    expect(isLineOriented(false)).toBe(true)
    expect(isLineOriented(true)).toBe(false)
  })

  it('trailing blank rows are dropped', () => {
    // They carry no information and cost vertical space, which on a phone is
    // the scarcest thing there is.
    expect(trimTrailingBlank(['a', 'b', '', '   ', ''])).toEqual(['a', 'b'])
  })

  it('a blank line between content is kept', () => {
    // It is a paragraph break, not padding.
    expect(trimTrailingBlank(['a', '', 'b'])).toEqual(['a', '', 'b'])
  })

  it('an entirely blank screen trims to nothing rather than throwing', () => {
    expect(trimTrailingBlank(['', '  ', ''])).toEqual([])
    expect(trimTrailingBlank([])).toEqual([])
  })
})

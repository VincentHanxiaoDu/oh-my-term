/**
 * Drawing a terminal in a browser.
 *
 * The decision this rests on: **the browser does not emulate a terminal.**
 * omt already has one emulator, in Rust, under test, and it is the one the
 * local TUI renders from. A second parser in JavaScript would be a second set
 * of answers about what the screen says — and the two would diverge on exactly
 * the input that is hard to parse, which is the input that matters.
 *
 * So the instance sends styled runs and this turns them into elements. There
 * is no VT state here, and there should never be any.
 */

/** A run of characters sharing one style, as the instance sends it. */
export interface StyledRun {
  text: string
  fg?: string
  bg?: string
  bold?: boolean
  italic?: boolean
  underline?: boolean
  inverse?: boolean
}

/** A screen, as the instance sends it. */
export interface Snapshot {
  rows: StyledRun[][]
  cols: number
  rows_count: number
  cursor: [number, number]
  cursor_visible: boolean
  alternate_screen: boolean
}

/** The CSS one run needs. */
export function styleOf(run: StyledRun): string {
  const parts: string[] = []
  // Inverse is applied by swapping, not by a filter: a filter also inverts the
  // glyph's antialiasing and the text comes out fringed.
  const fg = run.inverse === true ? run.bg : run.fg
  const bg = run.inverse === true ? run.fg : run.bg
  // When inverse swaps in an absent colour, the default has to become explicit
  // or the swap silently does nothing — which is how a selected line renders
  // identically to an unselected one.
  parts.push(`color:${fg ?? (run.inverse === true ? 'var(--omt-bg)' : 'inherit')}`)
  parts.push(`background:${bg ?? (run.inverse === true ? 'var(--omt-fg)' : 'transparent')}`)
  if (run.bold === true) {
    parts.push('font-weight:700')
  }
  if (run.italic === true) {
    parts.push('font-style:italic')
  }
  if (run.underline === true) {
    parts.push('text-decoration:underline')
  }
  return parts.join(';')
}

/** How many characters wide a snapshot's row is. */
export function widthOf(row: readonly StyledRun[]): number {
  return row.reduce((n, run) => n + [...run.text].length, 0)
}

/**
 * Whether a snapshot can be drawn at all.
 *
 * A client that renders a malformed snapshot shows a plausible-looking screen
 * that is not what the terminal says, which is worse than showing nothing:
 * somebody answers a prompt based on it.
 */
export function isCoherent(snapshot: Snapshot): boolean {
  if (snapshot.rows.length !== snapshot.rows_count) {
    return false
  }
  const [row, col] = snapshot.cursor
  return row < snapshot.rows_count && col <= snapshot.cols
}

/**
 * Draw a screen into an element.
 *
 * One element per row and one per run — not one per cell. A cell-per-element
 * grid at 50×30 is 1500 nodes rebuilt on every frame, which a phone spends
 * more time laying out than the terminal spends producing.
 */
export function paint(host: HTMLElement, snapshot: Snapshot): void {
  host.textContent = ''
  if (!isCoherent(snapshot)) {
    host.textContent = 'that screen arrived malformed and is not being shown'
    return
  }
  host.style.setProperty('--omt-cols', String(snapshot.cols))

  snapshot.rows.forEach((runs, index) => {
    const line = document.createElement('div')
    line.className = 'line'
    for (const run of runs) {
      const span = document.createElement('span')
      span.style.cssText = styleOf(run)
      span.textContent = run.text
      line.append(span)
    }
    if (snapshot.cursor_visible && snapshot.cursor[0] === index) {
      const cursor = document.createElement('span')
      cursor.className = 'cursor'
      cursor.style.left = `calc(${snapshot.cursor[1]}ch)`
      line.append(cursor)
    }
    host.append(line)
  })
}

/**
 * Whether a repaint is worth doing.
 *
 * A terminal producing output changes a few rows and leaves the rest alone.
 * Repainting everything works and is what makes a browser terminal feel slow,
 * so an identical screen costs nothing.
 */
export function changed(previous: Snapshot | null, next: Snapshot): boolean {
  if (previous === null) {
    return true
  }
  if (
    previous.cursor[0] !== next.cursor[0] ||
    previous.cursor[1] !== next.cursor[1] ||
    previous.cursor_visible !== next.cursor_visible
  ) {
    return true
  }
  return JSON.stringify(previous.rows) !== JSON.stringify(next.rows)
}

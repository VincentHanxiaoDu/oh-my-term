/**
 * Fitting a terminal to a phone.
 *
 * The decision that shapes this: **reflow, not letterbox.** A phone is 40–55
 * columns at a legible size, and a program that drew an 80-column frame is
 * unreadable either way — but a resized grid is one the program can redraw
 * correctly, where a scaled bitmap is one nobody can read without pinching.
 * Every serious mobile terminal resizes and sends SIGWINCH.
 */
/** The narrowest grid worth asking for. */
export const MIN_COLS = 20;
/** The shortest. */
export const MIN_ROWS = 4;
/** The widest — beyond this a phone is not the right surface anyway. */
export const MAX_COLS = 500;
/** The tallest. */
export const MAX_ROWS = 200;
/**
 * How large a grid fits.
 *
 * Clamped at both ends. A window manager really does report zero mid-drag, and
 * a grid of zero columns makes every downstream width calculation a division by
 * zero; an unbounded one lets a stray measurement ask for a million columns.
 */
export function fit(viewport) {
    // A cell of zero would divide by zero — which happens when this is called
    // before the font has loaded, and is the single commonest bug in browser
    // terminals.
    if (viewport.cellWidth <= 0 || viewport.cellHeight <= 0) {
        return { cols: MIN_COLS, rows: MIN_ROWS };
    }
    const cols = Math.floor(viewport.width / viewport.cellWidth);
    const rows = Math.floor(viewport.height / viewport.cellHeight);
    return {
        cols: clamp(cols, MIN_COLS, MAX_COLS),
        rows: clamp(rows, MIN_ROWS, MAX_ROWS),
    };
}
/**
 * Whether a new fit is worth telling the instance about.
 *
 * A resize is a `SIGWINCH` and a full redraw on the far side, so sending one
 * per pixel of a drag would make the program redraw sixty times a second. The
 * threshold is one whole cell, because anything smaller cannot change what is
 * displayed.
 */
export function worthResizing(current, next) {
    return current.cols !== next.cols || current.rows !== next.rows;
}
/** The font size that would fit a target column count. */
export function sizeForColumns(width, columns, advanceRatio = 0.6) {
    if (columns <= 0 || advanceRatio <= 0) {
        return 12;
    }
    return Math.max(6, Math.floor(width / columns / advanceRatio));
}
/**
 * Whether a screen is worth rendering as lines at all.
 *
 * While a full-screen program is drawing, every cell belongs to it and a
 * line-oriented transcript view of it is nonsense. A client that rendered one
 * anyway looks like omt is broken rather than like vim is running.
 */
export function isLineOriented(alternateScreen) {
    return !alternateScreen;
}
/** Trailing blank rows carry no information and cost vertical space. */
export function trimTrailingBlank(rows) {
    const out = [...rows];
    while (out.length > 0 && out.at(-1)?.trim() === '') {
        out.pop();
    }
    return out;
}
function clamp(value, low, high) {
    return Math.min(Math.max(value, low), high);
}

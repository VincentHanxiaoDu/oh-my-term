/**
 * omt's mark.
 *
 * Two states, because the product is two things at once. A session with no
 * agent in it is a terminal, and the mark is a prompt: the one glyph that reads
 * as "terminal" at sixteen pixels in a browser tab. A session with an agent in
 * it is not a terminal any more — it is a conversation that happens to have a
 * terminal underneath — and the mark says so.
 *
 * Drawn rather than fetched: an inline SVG inherits `currentColor`, so the mark
 * is the accent in the bar and the tab colour in the favicon without a second
 * asset, and it cannot fail to load.
 */
/**
 * Which mark a session should wear.
 *
 * From what the session *is*, not from what it is called: a shell named
 * "claude" is still a shell, and a session running an agent is an agent session
 * whether or not anybody named it well.
 */
export function markFor(threads) {
    return threads > 0 ? 'agent' : 'terminal';
}
/**
 * The mark, as an SVG string.
 *
 * A prompt chevron and a block cursor for a terminal; the same chevron with the
 * cursor replaced by a filled dot for an agent — the cursor is where *you*
 * type, and in an agent session something else is doing the typing.
 */
export function markSvg(kind, size = 24) {
    const glyph = kind === 'agent'
        ? // The dot sits where the cursor would be: same rhythm, different
            // meaning, so the two marks read as one family rather than two logos.
            '<circle cx="16.5" cy="15" r="3.2" fill="currentColor"/>'
        : '<rect x="13.5" y="10.5" width="3.4" height="9" fill="currentColor"/>';
    return [
        `<svg viewBox="0 0 24 24" width="${size}" height="${size}"`,
        ' xmlns="http://www.w3.org/2000/svg" aria-hidden="true" focusable="false">',
        // The chevron, drawn as a stroke so it keeps its weight at any size.
        '<path d="M5 7.5 L10 12 L5 16.5" fill="none" stroke="currentColor"',
        ' stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>',
        glyph,
        '</svg>',
    ].join('');
}
/** The mark as an element, ready to put in a bar. */
export function markElement(kind, size = 20) {
    const holder = document.createElement('span');
    holder.className = `mark mark-${kind}`;
    holder.innerHTML = markSvg(kind, size);
    // Named for a screen reader, which would otherwise announce nothing at all
    // for the one element that says what this screen is.
    holder.setAttribute('role', 'img');
    holder.setAttribute('aria-label', kind === 'agent' ? 'agent session' : 'terminal');
    return holder;
}
/**
 * The favicon, as a data URL.
 *
 * A data URL rather than a file, so the tab icon follows the same adaptive rule
 * as the mark in the bar: a phone with six tabs open should be able to tell the
 * one with an agent waiting from the five with shells in them.
 */
export function faviconUrl(kind, colour) {
    const svg = markSvg(kind, 32).replace('<svg ', `<svg style="color:${colour}" `);
    // Encoded rather than base64: shorter, and readable in a devtools inspector
    // when somebody is working out why the tab is wrong.
    return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

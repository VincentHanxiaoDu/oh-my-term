/**
 * The subagent grid.
 *
 * Claude Code's desktop client has a subagent switcher; its mobile client shows
 * one session at a time even when five agents are running. Closing that gap on
 * a phone is what this is for.
 *
 * **What steering honestly means here.** A running subagent has no free-text
 * input channel — the parent agent drives it, and nothing exposes a prompt for
 * one. What is actionable is answering a card that subagent raised, and
 * interrupting. Rendering a text box omt cannot deliver into would be the same
 * mistake as offering an answer button for an undeliverable card.
 */
/** Which tone a state reads as. */
export function toneOf(state) {
    switch (state.state) {
        case 'blocked':
            return 'blocked';
        case 'working':
            return 'working';
        case 'idle':
            return 'idle';
        case 'exited':
            return 'finished';
        case 'starting':
        case 'unknown':
            return 'unknown';
    }
}
const GLYPHS = {
    // Filled and distinct in shape, not only in colour.
    blocked: '●',
    working: '◐',
    idle: '○',
    finished: '✓',
    unknown: '·',
};
/** Build the cell for one thread. */
export function cellFor(thread) {
    const tone = toneOf(thread.state);
    const what = thread.label ?? (thread.is_subagent ? 'subagent' : 'main thread');
    const suffix = tone === 'blocked'
        ? 'needs you'
        : tone === 'working'
            ? 'working'
            : tone === 'finished'
                ? 'finished'
                : tone === 'idle'
                    ? 'waiting'
                    : 'no signal';
    return {
        thread,
        tone,
        glyph: GLYPHS[tone],
        label: `${what} — ${suffix}`,
        // Only a blocked thread has something to do *now*. A working one is
        // openable, but tapping it must not look like it will change anything.
        actionable: tone === 'blocked',
    };
}
/**
 * The grid, in the order it should be drawn.
 *
 * Blocked threads first. A grid sorted by spawn order buries the one that needs
 * you behind four that do not, which on a phone means scrolling to find the
 * only cell that matters.
 */
export function grid(threads) {
    const order = {
        blocked: 0,
        working: 1,
        idle: 2,
        unknown: 3,
        finished: 4,
    };
    return [...threads]
        .map(cellFor)
        .sort((a, b) => order[a.tone] - order[b.tone] ||
        // Within a tone, a stable tiebreak: cells that reshuffle between two
        // renders are cells the user mistaps.
        Number(a.thread.is_subagent) - Number(b.thread.is_subagent) ||
        a.thread.id.localeCompare(b.thread.id));
}
/** What the header says above the grid. */
export function summarize(threads) {
    const cells = threads.map(cellFor);
    const blocked = cells.filter((c) => c.tone === 'blocked').length;
    const working = cells.filter((c) => c.tone === 'working').length;
    if (blocked > 0) {
        // The one number worth leading with.
        return `${blocked} of ${threads.length} need you`;
    }
    if (working > 0) {
        return `${working} of ${threads.length} working`;
    }
    return threads.length === 0 ? 'nothing running' : `${threads.length} idle`;
}
/**
 * What a tap offers, most useful first.
 *
 * Deliberately never includes a prompt: a running subagent has no input channel
 * to deliver one into.
 */
export function actionsFor(thread) {
    const actions = thread.open_interactions.map((interaction) => ({
        action: 'answer',
        interaction,
    }));
    actions.push({ action: 'open-transcript', thread: thread.id });
    return actions;
}

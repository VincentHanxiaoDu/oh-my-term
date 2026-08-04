/**
 * Making a terminal usable with a thumb.
 *
 * A phone keyboard has no Esc, no Ctrl, no arrows and no Tab. Without the key
 * bar below, a terminal on a phone is read-only in practice — which is
 * literally the open bug that makes VS Code's mobile terminal unusable.
 */
/**
 * The bar, left to right.
 *
 * `Esc` first because it is both the most pressed and the most missing. `Ctrl`
 * latches rather than being held — a modifier that must be held cannot be used
 * one-handed. The last four are the characters a software keyboard buries two
 * layers deep and that appear in almost every command; Termux ships `-` in its
 * seven-key default for exactly that reason.
 */
export const KEY_BAR = [
    { label: 'Esc', action: { send: '\x1b' } },
    { label: 'Tab', action: { send: '\t' } },
    { label: 'Ctrl', action: { latch: 'ctrl' } },
    { label: '↑', action: { send: '\x1b[A' } },
    { label: '↓', action: { send: '\x1b[B' } },
    { label: '←', action: { send: '\x1b[D' } },
    { label: '→', action: { send: '\x1b[C' } },
    { label: '/', action: { send: '/' } },
    { label: '-', action: { send: '-' } },
    { label: '|', action: { send: '|' } },
    { label: '~', action: { send: '~' } },
];
/** The second row, behind a swipe — tuned to agents rather than shells. */
export const KEY_BAR_SECONDARY = [
    { label: '^C', action: { send: '\x03' } },
    { label: '^D', action: { send: '\x04' } },
    { label: '^R', action: { send: '\x12' } },
    { label: 'PgUp', action: { send: '\x1b[5~' } },
    { label: 'PgDn', action: { send: '\x1b[6~' } },
];
/** What a latched modifier does to the next key. */
export function applyLatch(latched, key) {
    if (latched === null || key.length !== 1) {
        return key;
    }
    if (latched === 'alt') {
        // A prefixed escape, which every terminal and every readline agrees on.
        return `\x1b${key}`;
    }
    const upper = key.toUpperCase();
    const code = upper.charCodeAt(0);
    // Control maps a letter into the C0 range — the same mapping that has made
    // Ctrl-C mean interrupt since before any of this existed.
    if (code >= 0x40 && code <= 0x5f) {
        return String.fromCharCode(code - 0x40);
    }
    return key;
}
/** How far a drag must go before it counts as one cell of movement. */
export const DRAG_STEP_PX = 24;
/**
 * Long-press-and-drag becomes arrow keys, accelerating with distance.
 *
 * The best gesture in any mobile terminal, and the reason: it makes history and
 * cursor movement usable without reaching for the bar at all. Acceleration
 * matters because scrolling back forty commands one press at a time is not
 * something anybody does twice.
 */
export function dragToArrows(dx, dy) {
    const horizontal = Math.abs(dx) > Math.abs(dy);
    const distance = horizontal ? Math.abs(dx) : Math.abs(dy);
    const steps = Math.floor(distance / DRAG_STEP_PX);
    if (steps === 0) {
        return null;
    }
    // Accelerating: the first few are one-to-one, then each further step counts
    // for more. A long drag should cross a long history.
    const repeat = steps <= 4 ? steps : 4 + (steps - 4) * 3;
    const direction = horizontal
        ? dx > 0
            ? 'right'
            : 'left'
        : dy > 0
            ? 'down'
            : 'up';
    return { kind: 'arrows', direction, repeat };
}
/** The bytes a gesture sends, if it sends any. */
export function gestureBytes(gesture) {
    switch (gesture.kind) {
        case 'arrows': {
            const code = { up: 'A', down: 'B', right: 'C', left: 'D' }[gesture.direction];
            return `\x1b[${code}`.repeat(gesture.repeat);
        }
        case 'tab':
            return '\t';
        // Scrolling, resizing and switching are the client's own business; sending
        // bytes for them would put input into a program that never asked for it.
        case 'scroll':
        case 'resize':
        case 'switch':
            return '';
    }
}
/**
 * How a card's options should be confirmed.
 *
 * A second gate **only** above a threshold. Requiring a hold for everything
 * manufactures the habituation the gate exists to prevent: Akhawe and Felt
 * measured 23–25% click-through on browser malware warnings, and found the
 * most frequently shown warning had the *lowest* adherence.
 */
export function confirmation(risk) {
    return risk === 'irreversible' ? 'hold' : 'tap';
}
/** How long a hold must last. */
export const HOLD_MS = 800;
/**
 * Whether an approval still applies to what the agent is asking.
 *
 * Bound to the card id **and** a hash of the exact command. If the request
 * changed between render and tap, the approval is rejected rather than applied
 * to the new one — the same rule EU payment authentication imposes, and for the
 * same reason.
 */
export function approvalStillValid(approved, current) {
    return approved.id === current.id && approved.commandHash === current.commandHash;
}
/**
 * Classify what an agent wants to do.
 *
 * Deliberately conservative and deliberately small. This decides only how hard
 * a confirmation is, never whether something is allowed — omt does not add a
 * permission model on top of the agent's own.
 */
export function riskOf(command) {
    const c = command.trim();
    const irreversible = [
        /^rm\s+-[rf]{1,2}\s+[/~]/,
        /^git\s+push\s+.*--force/,
        /^git\s+reset\s+--hard/,
        /^(mkfs|dd\s+if=.*of=\/dev\/)/,
        /^(drop|truncate)\s+(table|database)/i,
        /^kubectl\s+delete/,
        /^terraform\s+(destroy|apply)/,
    ];
    if (irreversible.some((r) => r.test(c))) {
        return 'irreversible';
    }
    const reversible = [/^rm\s/, /^git\s+(checkout|restore|clean)/, /^mv\s/];
    return reversible.some((r) => r.test(c)) ? 'reversible' : 'ordinary';
}

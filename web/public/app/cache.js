/**
 * What the browser is allowed to keep, and what goes first when it cannot.
 *
 * `docs/architecture/23-identity-and-devices.md` §9.1.
 *
 * The budget exists because a browser that runs out of storage does not ask —
 * it throws on the next write, and the write that fails is whichever one
 * happened to be next rather than the least important. Trimming on our own
 * terms means we choose what is lost; leaving it to the browser means the
 * device un-pairs itself because the identity record was the unlucky one.
 *
 * Which is why some stores are never evicted at all. Dropping `identity` or
 * `keys` does not free much and costs the device its pairing — the user is
 * then holding a phone that has forgotten which machine it belongs to, with no
 * indication why.
 *
 * `terminal` goes first among the rest, because it is by far the largest and by
 * far the cheapest to lose: it is a cache of something the instance still has,
 * and refetching it costs one round trip.
 */
/** The stores, in the order they are trimmed. */
export const STORES = [
    // Never evicted: losing either un-pairs the device.
    { name: 'identity', cap: 8 * 1024, eviction: 'never' },
    { name: 'keys', cap: 0, eviction: 'never' },
    { name: 'credentials', cap: 16 * 1024, eviction: 'never' },
    { name: 'prefs', cap: 32 * 1024, eviction: 'never' },
    // Trimmed, cheapest to lose first.
    { name: 'terminal', cap: 4 * 1024 * 1024, eviction: 'lru' },
    { name: 'sessions', cap: 512 * 1024, eviction: 'lru' },
    { name: 'interactions', cap: 256 * 1024, eviction: 'age' },
    { name: 'instances', cap: 256 * 1024, eviction: 'lru' },
];
/** The most the client will hold across every store. */
export const BUDGET = 8 * 1024 * 1024;
/** The order stores are trimmed in when the budget is exceeded. */
export const TRIM_ORDER = ['terminal', 'sessions', 'interactions', 'instances'];
/** How much is held in total. */
export function total(usage) {
    return Object.values(usage).reduce((sum, bytes) => sum + bytes, 0);
}
/**
 * What to trim to get back under budget.
 *
 * Returns the reductions in order. Nothing that cannot be evicted is ever
 * named, even when it is the largest thing there — a plan that suggested
 * dropping the identity record would be a plan that un-pairs the device to save
 * eight kilobytes.
 */
export function planTrim(usage, budget = BUDGET) {
    let over = total(usage) - budget;
    if (over <= 0)
        return [];
    const plan = [];
    for (const name of TRIM_ORDER) {
        if (over <= 0)
            break;
        const held = usage[name] ?? 0;
        if (held <= 0)
            continue;
        const bytes = Math.min(held, over);
        plan.push({ store: name, bytes });
        over -= bytes;
    }
    return plan;
}
/**
 * Whether the budget can be met at all by trimming.
 *
 * False when the never-evicted stores alone exceed it. Reported rather than
 * attempted, because the alternative is a loop that trims everything it may,
 * finds itself still over, and trims it again next write — burning the device's
 * battery to no effect.
 */
export function canFitWithinBudget(usage, budget = BUDGET) {
    const permanent = STORES.filter((s) => s.eviction === 'never').map((s) => s.name);
    const fixed = permanent.reduce((sum, name) => sum + (usage[name] ?? 0), 0);
    return fixed <= budget;
}
/** Whether a store may be trimmed at all. */
export function isEvictable(name) {
    return STORES.find((s) => s.name === name)?.eviction !== 'never';
}
/**
 * Whether a store is allowed to hold terminal content.
 *
 * Only `terminal`. The session list holds titles, workspaces and counts and no
 * output at all — a cached session row is a thing somebody's colleague may see
 * over their shoulder on a lock screen, and a title is a very different
 * exposure from the last thing the shell printed.
 */
export function mayHoldTerminalContent(name) {
    return name === 'terminal';
}

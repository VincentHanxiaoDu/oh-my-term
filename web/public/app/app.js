/**
 * The screens.
 *
 * Deliberately no framework. This is a few hundred lines of DOM against a store
 * that already holds every rule, and a framework here would be a dependency
 * whose upgrade cadence outlives the code it renders.
 *
 * The order of the screens is the argument of the whole client: the roster
 * first, because a phone is used for ninety seconds because something buzzed,
 * and a terminal that opens to a wall of text has already failed that.
 */
import { KEY_BAR, KEY_BAR_SECONDARY, applyLatch, confirmation, riskOf } from './touch.js';
import { trimTrailingBlank } from './screen.js';
/** How a session row reads. */
export function renderSessionRow(session) {
    const blocked = session.state === 'blocked';
    return {
        text: session.title,
        className: blocked ? 'row blocked' : 'row',
        label: `${session.title} — ${blocked ? 'needs you' : session.state}`,
    };
}
/**
 * The roster, ordered.
 *
 * Blocked first, and never by name or recency: spawn order buries the one row
 * that matters behind four that do not, which on a phone means scrolling to
 * find the only thing you opened the app for.
 */
export function orderRoster(rows) {
    const rank = (s) => s === 'blocked' ? 0 : s === 'working' ? 1 : s === 'idle' ? 2 : 3;
    return [...rows].sort((a, b) => rank(a.state) - rank(b.state) || a.title.localeCompare(b.title));
}
/** The header above the roster — one number. */
export function rosterHeader(state) {
    if (state.connection !== 'connected') {
        return state.refusal ?? `${state.connection}…`;
    }
    if (state.needsYou > 0) {
        return `${state.needsYou} of ${state.sessions.length} need you`;
    }
    return state.sessions.length === 0
        ? 'no sessions'
        : `${state.sessions.length} running`;
}
/**
 * Lay out a card's options.
 *
 * The agent's own options, verbatim and in its order — omt neither adds,
 * removes nor reorders them, because the indices are how the answer is
 * delivered. What omt adds is only *how hard each one is to choose*.
 */
export function layOutCard(options, command) {
    const risk = command === undefined ? 'ordinary' : riskOf(command);
    return options.map((label, index) => ({
        label,
        // Only the affirmative options inherit the command's risk. Making "Deny"
        // hard to press would push people toward the dangerous answer, which is
        // precisely backwards.
        confirm: isAffirmative(label) ? confirmation(risk) : 'tap',
        isDefault: index === 0,
    }));
}
function isAffirmative(label) {
    return /^(yes|allow|approve|ok|confirm|proceed|continue)/i.test(label.trim());
}
/** Whether a card can be acted on here, and why not if not. */
export function cardStatus(interaction) {
    if (interaction.deliverable.kind === 'none') {
        return { answerable: false, why: interaction.deliverable.reason };
    }
    if (interaction.state.state !== 'open') {
        return { answerable: false, why: 'it has already been answered' };
    }
    return { answerable: true };
}
/** A tiny renderer, so the shell has no framework and no build step. */
export class App {
    #store;
    #root;
    #route = { screen: 'roster' };
    #latched = null;
    constructor(store, root) {
        this.#store = store;
        this.#root = root;
        store.subscribe(() => this.render());
    }
    /** Go somewhere. */
    go(route) {
        this.#route = route;
        this.render();
    }
    /** Which screen is showing. */
    get route() {
        return this.#route;
    }
    /** A key from the accessory bar. */
    pressBarKey(index, secondary = false) {
        const bar = secondary ? KEY_BAR_SECONDARY : KEY_BAR;
        const key = bar[index];
        if (!key) {
            return null;
        }
        if ('latch' in key.action) {
            // Tapping a latched modifier again releases it — otherwise a mis-tap
            // arms Ctrl and the next letter does something unexpected.
            this.#latched = this.#latched === key.action.latch ? null : key.action.latch;
            return null;
        }
        const bytes = applyLatch(this.#latched, key.action.send);
        this.#latched = null;
        return bytes;
    }
    /** Which modifier is armed, for the bar to show. */
    get latched() {
        return this.#latched;
    }
    /** Draw. */
    render() {
        const state = this.#store.state;
        this.#root.textContent = '';
        this.#root.append(this.#header(state));
        switch (this.#route.screen) {
            case 'roster':
                this.#root.append(this.#roster(state));
                break;
            case 'session':
                this.#root.append(this.#session(this.#route.session));
                break;
            case 'terminal':
                this.#root.append(this.#terminal(this.#route.session));
                break;
            case 'card':
                this.#root.append(this.#card(state, this.#route.interaction));
                break;
        }
        for (const problem of state.problems) {
            const note = document.createElement('p');
            note.className = problem.transient ? 'problem transient' : 'problem';
            note.textContent = problem.message;
            this.#root.append(note);
        }
    }
    #header(state) {
        const h = document.createElement('h1');
        h.textContent = rosterHeader(state);
        return h;
    }
    #roster(state) {
        const list = document.createElement('ul');
        for (const session of orderRoster(state.sessions.map((s) => ({ ...s, title: s.title })))) {
            const row = renderSessionRow(session);
            const item = document.createElement('li');
            const button = document.createElement('button');
            button.className = row.className;
            button.textContent = row.text;
            button.setAttribute('aria-label', row.label);
            button.addEventListener('click', () => {
                void this.#store.loadThreads(session.id);
                this.go({ screen: 'session', session: session.id });
            });
            item.append(button);
            list.append(item);
        }
        return list;
    }
    #session(session) {
        const box = document.createElement('div');
        const summary = document.createElement('p');
        summary.textContent = this.#store.summaryFor(session);
        box.append(summary);
        const gridEl = document.createElement('div');
        gridEl.className = 'grid';
        for (const cell of this.#store.gridFor(session)) {
            const el = document.createElement('button');
            el.className = `cell ${cell.tone}`;
            el.textContent = cell.glyph;
            el.setAttribute('aria-label', cell.label);
            el.disabled = !cell.actionable && cell.tone !== 'working';
            const first = cell.thread.open_interactions[0];
            if (first !== undefined) {
                el.addEventListener('click', () => this.go({ screen: 'card', interaction: first }));
            }
            gridEl.append(el);
        }
        box.append(gridEl);
        return box;
    }
    #terminal(session) {
        const pre = document.createElement('pre');
        pre.className = 'terminal';
        const rows = this.#store.state.sessions.find((s) => s.id === session);
        pre.textContent = rows ? '' : 'no such session';
        return pre;
    }
    #card(state, id) {
        const box = document.createElement('div');
        const interaction = state.open.find((i) => i.id === id);
        if (!interaction) {
            box.textContent = 'that card is no longer open';
            return box;
        }
        const status = cardStatus(interaction);
        if (!status.answerable) {
            // Shown read-only with the reason and a route to the terminal, rather
            // than a button that silently does nothing.
            const why = document.createElement('p');
            why.className = 'readonly';
            why.textContent = `omt cannot answer this here: ${status.why ?? 'unknown'}`;
            box.append(why);
        }
        return box;
    }
}

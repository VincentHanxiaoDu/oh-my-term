/**
 * The screens.
 *
 * Composed from `view.ts` rather than hand-built, so two screens cannot
 * disagree about what a row is. No framework: the store and the terminal
 * renderer are the hard parts and are tested without a DOM, and a diffing
 * algorithm for a list of six things is not worth a dependency whose upgrade
 * cadence outlives the code it renders.
 *
 * The order of the screens is the argument of the whole client: the roster
 * first, because a phone is picked up for ninety seconds because something
 * buzzed, and a terminal that opens to a wall of text has already failed that.
 */
import { KEY_BAR, KEY_BAR_SECONDARY, applyLatch, confirmation, riskOf } from './touch.js';
import { changed, paint } from './terminal.js';
import { Navigator, hasBack, titleOf } from './nav.js';
import { appBar, el, renderState, row, stateOf } from './view.js';
/** How often the terminal screen asks for a new picture. */
export const POLL_MS = 150;
/** Whether this row is the reason the phone buzzed. */
export function needsYou(state) {
    return state === 'blocked';
}
/**
 * Order the roster.
 *
 * Blocked first, and never by spawn order: that buries the one row that matters
 * behind four that do not, which on a phone means scrolling to find the only
 * thing you opened the app for. Ties break by name so the list does not shuffle
 * between glances.
 */
export function orderRoster(rows) {
    const rank = (s) => (s === 'blocked' ? 0 : s === 'working' ? 1 : s === 'idle' ? 2 : 3);
    return [...rows].sort((a, b) => rank(a.state) - rank(b.state) || a.title.localeCompare(b.title));
}
/**
 * What the roster's bar says under the title.
 *
 * Derived from the same rows the body renders. The bug that started this
 * rebuild was a header counting one thing and a body counting another, both on
 * screen, disagreeing — with no way for a user to tell which was right.
 */
export function rosterSubtitle(rows) {
    const blocked = rows.filter((r) => needsYou(r.state)).length;
    if (blocked > 0) {
        return `${blocked} of ${rows.length} need you`;
    }
    return rows.length === 1 ? '1 session' : `${rows.length} sessions`;
}
/**
 * Which screen a session opens.
 *
 * Decided from what the session *is*. Routing everything to the thread grid —
 * which the old client did — shows an empty screen for a plain shell, which is
 * the one thing somebody opens a terminal for.
 */
export function screenFor(session, threads) {
    return threads > 0 || session.state === 'blocked' ? 'threads' : 'terminal';
}
/**
 * Lay out a card's options.
 *
 * The agent's own options, verbatim and in its order — omt neither adds,
 * removes nor reorders them, because the labels are how an answer is delivered.
 * What omt adds is only *how hard each one is to choose*.
 */
export function layOutCard(options, command) {
    const risk = command === undefined ? 'ordinary' : riskOf(command);
    return options.map((label, index) => ({
        label,
        // Only affirmative options inherit the command's risk. Making "Deny" hard
        // to press would push people toward the dangerous answer.
        confirm: isAffirmative(label) ? confirmation(risk) : 'tap',
        isDefault: index === 0,
    }));
}
function isAffirmative(label) {
    return /^(yes|allow|approve|ok|confirm|proceed|continue)/i.test(label.trim());
}
/** Whether a card can be acted on here, and why not if not. */
export function cardOffer(card) {
    if (card.deliverable === 'none') {
        return {
            answerable: false,
            why: card.not_deliverable_because ?? 'omt has no channel to answer this one',
            options: [],
        };
    }
    if (card.state !== 'open') {
        return { answerable: false, why: 'it has already been answered', options: [] };
    }
    // A permission card's prompt is the command, and the command is what decides
    // how hard "yes" is to press. Running the heuristic over a *question* would
    // fire on the word "delete" in ordinary prose.
    const command = card.kind === 'permission' ? card.prompt : undefined;
    return { answerable: true, options: layOutCard(card.options, command) };
}
/** The client. */
export class App {
    #store;
    #root;
    #nav;
    #screen = null;
    #epoch = 0;
    #poll = null;
    #latched = null;
    #cards;
    #threads = {};
    /** Per-screen, so one screen's failure never appears under another's content. */
    #errors = {};
    constructor(store, root, nav = new Navigator()) {
        this.#store = store;
        this.#root = root;
        this.#nav = nav;
        store.subscribe(() => this.render());
        nav.subscribe(() => this.onRouteChange());
    }
    /** Where we are. */
    get route() {
        return this.#nav.route;
    }
    /** Go somewhere. */
    go(route) {
        this.#nav.push(route);
    }
    /** Which modifier is armed, for the key bar to show. */
    get latched() {
        return this.#latched;
    }
    /** A key from the accessory bar. */
    pressBarKey(index, secondary = false) {
        const key = (secondary ? KEY_BAR_SECONDARY : KEY_BAR)[index];
        if (!key) {
            return null;
        }
        if ('latch' in key.action) {
            // Tapping an armed modifier releases it — otherwise a mis-tap arms Ctrl
            // and the next letter does something unexpected.
            this.#latched = this.#latched === key.action.latch ? null : key.action.latch;
            this.render();
            return null;
        }
        const bytes = applyLatch(this.#latched, key.action.send);
        this.#latched = null;
        this.render();
        return bytes;
    }
    /** Type into the session on screen. */
    async type(text) {
        const route = this.#nav.route;
        if (route.screen !== 'terminal') {
            return;
        }
        try {
            await this.#store.call('session.write', route.session, text, this.#epoch);
            await this.refreshTerminal(route.session);
        }
        catch (e) {
            this.fail('terminal', e);
        }
    }
    /** Draw. */
    render() {
        const route = this.#nav.route;
        this.#root.textContent = '';
        switch (route.screen) {
            case 'roster':
                this.#renderRoster();
                break;
            case 'terminal':
                this.#renderTerminal(route.session);
                break;
            case 'threads':
                this.#renderThreads(route.session);
                break;
            case 'card':
                this.#renderCard(route.interaction);
                break;
        }
    }
    /** Load whatever the new screen needs. */
    onRouteChange() {
        this.stopPolling();
        const route = this.#nav.route;
        // The error belonged to the screen we left. Keeping it would put a failure
        // from one screen under another's content, which is what the old client did.
        delete this.#errors[route.screen];
        if (route.screen === 'terminal') {
            this.#screen = null;
            void this.refreshTerminal(route.session);
            this.#poll = setInterval(() => void this.refreshTerminal(route.session), POLL_MS);
        }
        if (route.screen === 'threads') {
            void this.#store.loadThreads(route.session);
        }
        if (route.screen === 'card' && this.#cards === undefined) {
            void this.loadCards();
        }
        this.render();
    }
    /** Stop asking for a screen nobody is looking at. */
    stopPolling() {
        if (this.#poll !== null) {
            clearInterval(this.#poll);
            this.#poll = null;
        }
    }
    /** Record a failure against the screen it happened on. */
    fail(screen, error) {
        const message = typeof error === 'object' && error !== null && 'message' in error
            ? String(error.message)
            : String(error);
        this.#errors[screen] = message;
        this.render();
    }
    /** Whatever went wrong on this screen, if anything. */
    errorFor(screen) {
        return this.#errors[screen];
    }
    /** Fetch the screen and redraw only if it moved. */
    async refreshTerminal(session) {
        try {
            const next = (await this.#store.call('session.snapshot', session));
            delete this.#errors.terminal;
            if (changed(this.#screen, next)) {
                this.#screen = next;
                this.render();
            }
        }
        catch (e) {
            this.stopPolling();
            this.fail('terminal', e);
        }
    }
    /** Re-read the cards waiting across every session. */
    async loadCards() {
        try {
            const out = (await this.#store.call('interaction.list'));
            this.#cards = out.interactions;
            delete this.#errors.card;
            this.render();
        }
        catch (e) {
            this.fail('card', e);
        }
    }
    /** The cards this client knows about. */
    get cards() {
        return this.#cards ?? [];
    }
    /** Start a session and go straight to its terminal. */
    async newSession(workspace) {
        try {
            const out = (await this.#store.call('session.create', workspace));
            const claim = (await this.#store.call('session.acquire', out.session));
            this.#epoch = claim.epoch;
            await this.#store.refresh();
            // Straight to the terminal, because the reason somebody pressed this is
            // that they want to type.
            this.go({ screen: 'terminal', session: out.session });
        }
        catch (e) {
            this.fail('roster', e);
        }
    }
    /** Answer a card. */
    async answer(interaction, option) {
        try {
            await this.#store.call('interaction.respond', interaction, option);
            this.#cards = undefined;
            await this.#store.refresh();
            this.#nav.back();
        }
        catch (e) {
            // A card that failed to send must not look answered, so the screen stays.
            this.fail('card', e);
        }
    }
    #shell(bar, body, keybar) {
        this.#root.append(bar, body);
        if (keybar) {
            this.#root.append(keybar);
        }
    }
    #renderRoster() {
        const view = this.#store.state;
        const rows = view.connection === 'connected' ? orderRoster(view.sessions) : undefined;
        const state = stateOf(rows, this.#errors.roster ?? view.refusal, {
            headline: 'Nothing running',
            detail: 'Start a session and it appears here.',
        });
        const workspace = view.workspaces[0];
        const bar = appBar({
            title: 'omt',
            // From the same rows the body renders, so the two cannot disagree.
            ...(rows ? { subtitle: rosterSubtitle(rows) } : {}),
            ...(workspace
                ? { action: { label: 'New session', onTap: () => void this.newSession(workspace.id) } }
                : {}),
        });
        const body = el('div', 'body');
        const waiting = this.cards.filter((c) => c.state === 'open');
        if (waiting.length > 0) {
            body.append(el('p', 'section', 'Needs you'));
            for (const card of waiting) {
                body.append(row({
                    title: card.prompt,
                    tone: 'needs-you',
                    label: `needs you: ${card.prompt}`,
                    onTap: () => this.go({ screen: 'card', interaction: card.id }),
                }));
            }
            body.append(el('p', 'section', 'Sessions'));
        }
        body.append(renderState(state, (sessions) => sessions.map((session) => row({
            title: session.title,
            meta: session.state,
            ...(needsYou(session.state)
                ? { tone: 'needs-you' }
                : session.state === 'running'
                    ? { tone: 'working' }
                    : {}),
            label: `${session.title} — ${needsYou(session.state) ? 'needs you' : session.state}`,
            onTap: () => {
                const threads = this.#threads[session.id] ?? 0;
                this.go({ screen: screenFor(session, threads), session: session.id });
            },
        }))));
        this.#shell(bar, body);
    }
    #renderTerminal(session) {
        const title = this.#store.state.sessions.find((s) => s.id === session)?.title;
        const bar = appBar({
            title: title ?? 'Terminal',
            onBack: () => this.#nav.back(),
        });
        const body = el('div', 'body');
        const error = this.#errors.terminal;
        if (error !== undefined) {
            body.append(renderState({ kind: 'error', headline: 'That session is not there', detail: error }, () => el('div')));
        }
        else if (this.#screen === null) {
            body.append(renderState({ kind: 'loading' }, () => el('div')));
        }
        else {
            const host = el('div', 'terminal');
            paint(host, this.#screen);
            body.append(host);
        }
        this.#shell(bar, body, error === undefined ? this.#keybar() : undefined);
    }
    #keybar() {
        const bar = el('div', 'keybar');
        KEY_BAR.forEach((key, index) => {
            const button = el('button', undefined, key.label);
            if ('latch' in key.action) {
                button.setAttribute('aria-pressed', String(this.#latched === key.action.latch));
            }
            button.addEventListener('click', () => {
                const bytes = this.pressBarKey(index);
                if (bytes !== null) {
                    void this.type(bytes);
                }
            });
            bar.append(button);
        });
        return bar;
    }
    #renderThreads(session) {
        const title = this.#store.state.sessions.find((s) => s.id === session)?.title;
        const bar = appBar({
            title: title ?? 'Agents',
            onBack: () => this.#nav.back(),
        });
        const cells = this.#store.gridFor(session);
        this.#threads[session] = cells.length;
        const state = stateOf(cells, this.#errors.threads, {
            headline: 'No agent here',
            detail: 'This session is a plain shell.',
        });
        const body = el('div', 'body');
        body.append(renderState(state, (grid) => {
            const wrapper = el('div', 'grid');
            for (const cell of grid) {
                const button = el('button', `cell ${cell.tone}`, cell.glyph);
                button.setAttribute('aria-label', cell.label);
                const first = cell.thread.open_interactions[0];
                if (first !== undefined) {
                    button.addEventListener('click', () => this.go({ screen: 'card', interaction: first }));
                }
                else {
                    button.disabled = true;
                }
                wrapper.append(button);
            }
            return wrapper;
        }));
        // A plain shell has no threads, and the way to what it does have is here.
        const open = el('button', 'tap primary');
        open.append(el('span', 'label', 'Open the terminal'));
        open.addEventListener('click', () => this.go({ screen: 'terminal', session }));
        body.append(open);
        this.#shell(bar, body);
    }
    #renderCard(id) {
        const card = this.#cards?.find((c) => c.id === id);
        const bar = appBar({ title: 'Needs you', onBack: () => this.#nav.back() });
        const body = el('div', 'body');
        if (this.#errors.card !== undefined) {
            body.append(renderState({ kind: 'error', headline: 'That did not send', detail: this.#errors.card }, () => el('div')));
            this.#shell(bar, body);
            return;
        }
        if (this.#cards === undefined) {
            body.append(renderState({ kind: 'loading' }, () => el('div')));
            this.#shell(bar, body);
            return;
        }
        if (card === undefined) {
            body.append(renderState({
                kind: 'empty',
                headline: 'Already answered',
                detail: 'Somebody got to this one first.',
            }, () => el('div')));
            this.#shell(bar, body);
            return;
        }
        body.append(el('p', 'prompt', card.prompt));
        const offer = cardOffer(card);
        if (!offer.answerable) {
            body.append(el('p', 'banner', `omt cannot answer this here: ${offer.why ?? 'unknown'}`));
            const open = el('button', 'tap primary');
            open.append(el('span', 'label', 'Open the terminal'));
            open.addEventListener('click', () => this.go({ screen: 'terminal', session: card.session }));
            body.append(open);
            this.#shell(bar, body);
            return;
        }
        for (const option of offer.options) {
            const button = el('button', option.confirm === 'hold' ? 'tap hold' : 'tap');
            button.append(el('span', 'label', option.label));
            // Said out loud: the difference between a tap and a hold is invisible
            // until somebody has already failed to press it.
            button.setAttribute('aria-label', option.confirm === 'hold' ? `${option.label} — press and hold` : option.label);
            button.addEventListener('click', () => void this.answer(card.id, option.label));
            body.append(button);
        }
        this.#shell(bar, body);
    }
}
export { hasBack, titleOf };

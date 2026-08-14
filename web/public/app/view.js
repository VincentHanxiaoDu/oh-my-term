/**
 * What a screen is showing, and the pieces every screen is built from.
 *
 * The bug that started the rebuild was a screen saying `1 running`, `nothing
 * running` and `no session sess_7ed…` at the same time — three independent
 * pieces of DOM appended in sequence, each true about a different question, and
 * no way for a user to know which to believe.
 *
 * `ScreenState` is a sum type so that cannot be written down. A screen is
 * loading *or* empty *or* failed *or* has content. Not four flags.
 */
import { markElement } from './mark.js';
/**
 * Decide a screen's state from what it knows.
 *
 * One place, so two screens cannot disagree about what "nothing yet" means. The
 * order matters and is the whole logic: an error outranks emptiness, because a
 * list that failed to load is not an empty list — telling somebody "no sessions"
 * when the request failed sends them looking for the wrong problem.
 */
export function stateOf(items, error, empty) {
    if (error !== undefined) {
        return { kind: 'error', headline: 'That did not work', detail: error };
    }
    if (items === undefined) {
        return { kind: 'loading' };
    }
    if (items.length === 0) {
        // Spread rather than always setting `detail`: this project turns on
        // `exactOptionalPropertyTypes`, where an explicit `undefined` and an absent
        // key are different things — which is the point of the setting.
        return { kind: 'empty', headline: empty.headline, ...detail(empty.detail) };
    }
    return { kind: 'content', value: items };
}
/** An optional detail, present only when there is one. */
function detail(text) {
    return text === undefined ? {} : { detail: text };
}
/** Make an element with a class and optional text. */
export function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className !== undefined) {
        node.className = className;
    }
    if (text !== undefined) {
        node.textContent = text;
    }
    return node;
}
/** The app bar: a back affordance when there is somewhere to go, and a title. */
export function appBar(options) {
    const bar = el('header', 'bar');
    if (options.onBack) {
        const back = el('button', 'icon-button', '‹');
        // Named, because "‹" is read aloud as nothing useful.
        back.setAttribute('aria-label', 'Back');
        back.addEventListener('click', options.onBack);
        bar.append(back);
    }
    if (options.mark) {
        bar.append(markElement(options.mark));
    }
    const heading = el('h1');
    if (options.onTitleTap) {
        // A real button, not an h1 with a click handler: this has to be reachable
        // by keyboard and announced as a control, and a heading is neither.
        const tap = el('button', 'title-button');
        tap.setAttribute('aria-label', `${options.title} — switch instance`);
        tap.append(document.createTextNode(options.title), el('span', 'chevron-down', '⌄'));
        heading.append(tap);
        tap.addEventListener('click', options.onTitleTap);
    }
    else {
        heading.append(document.createTextNode(options.title));
    }
    if (options.subtitle !== undefined) {
        const line = el('span', 'subtitle', options.subtitle);
        // The accent, driven from the same string that states the fact. A separate
        // flag would be a second place for "is anything waiting" to live, and two
        // places is how a headline and its colour come to disagree.
        if (/waiting on you/.test(options.subtitle)) {
            line.dataset['waiting'] = 'yes';
        }
        heading.append(line);
    }
    bar.append(heading);
    if (options.action) {
        const action = el('button', 'icon-button', '+');
        action.setAttribute('aria-label', options.action.label);
        action.addEventListener('click', options.action.onTap);
        bar.append(action);
    }
    return bar;
}
/** A row in a list: a status dot, a label, and a chevron if it leads somewhere. */
export function row(options) {
    const button = el('button', 'tap');
    button.setAttribute('aria-label', options.label);
    const dot = el('span', options.tone ? `dot ${options.tone}` : 'dot');
    dot.setAttribute('aria-hidden', 'true');
    button.append(dot);
    const label = el('span', 'label');
    label.append(el('span', 'title', options.title));
    if (options.meta !== undefined) {
        label.append(el('span', 'meta', options.meta));
    }
    button.append(label);
    if (options.leadsSomewhere !== false) {
        const chevron = el('span', 'chevron', '›');
        chevron.setAttribute('aria-hidden', 'true');
        button.append(chevron);
    }
    if (options.onTap) {
        button.addEventListener('click', options.onTap);
    }
    else {
        button.disabled = true;
    }
    return button;
}
/** Render whichever single state a screen is in. */
export function renderState(state, content) {
    switch (state.kind) {
        case 'loading': {
            const box = el('div', 'state');
            const spinner = el('div', 'spinner');
            spinner.setAttribute('role', 'status');
            spinner.setAttribute('aria-label', 'Loading');
            box.append(spinner);
            return box;
        }
        case 'empty':
        case 'error': {
            const box = el('div', state.kind === 'error' ? 'state error' : 'state');
            box.append(el('p', 'headline', state.headline));
            if (state.detail !== undefined) {
                box.append(el('p', 'detail', state.detail));
            }
            return box;
        }
        case 'content': {
            const box = el('div');
            const rendered = content(state.value);
            box.append(...(Array.isArray(rendered) ? rendered : [rendered]));
            return box;
        }
    }
}
/**
 * A screen with nothing on it, and what to do about that.
 *
 * An empty state is an invitation, not a report. "Nothing here" tells somebody
 * what they can already see; the second line is the part that earns the space.
 */
export function empty(title, detail) {
    const box = el('div', 'empty');
    box.append(el('h2', 'empty-title', title), el('p', 'empty-detail', detail));
    return box;
}

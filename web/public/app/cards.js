/**
 * The four kinds of thing an agent can ask, rendered natively.
 *
 * `docs/architecture/08-web-client.md` §5. A card is assembled from the agent's
 * *own* options, verbatim and in its order — omt neither adds, removes nor
 * reorders them, because the labels are how an answer is delivered. What omt
 * adds is only how hard each one is to press.
 */
import { el } from './view.js';
import { HOLD_MS } from './touch.js';
/**
 * Make a button that has to be held.
 *
 * The class alone was a lie: `confirm: 'hold'` rendered a different-looking
 * button that a single tap still activated, so an irreversible approval was one
 * accidental thumb away — on a device people use while walking.
 *
 * Pointer events rather than touch or mouse: one code path covers a finger, a
 * pencil and a mouse, and the three do not need three implementations that
 * drift.
 */
export function holdToConfirm(button, onConfirmed, holdMs = HOLD_MS) {
    let timer = null;
    const cancel = () => {
        if (timer !== null) {
            clearTimeout(timer);
            timer = null;
        }
        button.classList.remove('holding');
    };
    button.addEventListener('pointerdown', (event) => {
        // The pointer is captured, so sliding a thumb off the button still ends the
        // hold here rather than leaving it armed with nothing listening.
        ;
        event.target.setPointerCapture?.(event.pointerId);
        button.classList.add('holding');
        timer = setTimeout(() => {
            cancel();
            onConfirmed();
        }, holdMs);
    });
    for (const ending of ['pointerup', 'pointercancel', 'pointerleave']) {
        button.addEventListener(ending, cancel);
    }
    // A click that did not come from a completed hold does nothing, and says so
    // rather than appearing broken.
    button.addEventListener('click', (event) => event.preventDefault());
}
/** The heading a card wears, by kind. */
export function cardTitle(kind) {
    switch (kind) {
        case 'permission':
            return 'Permission';
        case 'plan_review':
            return 'Review this plan';
        case 'text':
            return 'Answer';
        default:
            return 'Needs you';
    }
}
/**
 * How the prompt is rendered.
 *
 * A permission card's prompt is a *command*, and a command shown as prose is a
 * command somebody skims. A choice card's prompt is a question, and a question
 * shown as code reads as something to run.
 */
export function promptElement(kind, prompt) {
    if (promptTag(kind) === 'pre') {
        const box = el('pre', 'prompt-command');
        box.textContent = prompt;
        return box;
    }
    return el('p', 'prompt', prompt);
}
/**
 * Which element a prompt is, given what is being asked.
 *
 * Split from the rendering so the decision can be tested without a DOM — the
 * decision is the part that can be wrong, and `document.createElement` is not.
 */
export function promptTag(kind) {
    return kind === 'permission' ? 'pre' : 'p';
}
/** Render a plan's steps, where the card carries one. */
export function planElement(steps) {
    const list = el('ol', 'plan');
    for (const step of steps) {
        const item = el('li', step.state ? `step ${step.state}` : 'step');
        item.textContent = step.title ?? '';
        list.append(item);
    }
    return list;
}

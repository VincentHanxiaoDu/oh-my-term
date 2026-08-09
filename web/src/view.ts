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

import { markElement } from './mark.js'

/** What one screen is showing right now. */
export type ScreenState<T> =
  | { kind: 'loading' }
  | { kind: 'empty'; headline: string; detail?: string }
  | { kind: 'error'; headline: string; detail?: string }
  | { kind: 'content'; value: T }

/**
 * Decide a screen's state from what it knows.
 *
 * One place, so two screens cannot disagree about what "nothing yet" means. The
 * order matters and is the whole logic: an error outranks emptiness, because a
 * list that failed to load is not an empty list — telling somebody "no sessions"
 * when the request failed sends them looking for the wrong problem.
 */
export function stateOf<T>(
  items: readonly T[] | undefined,
  error: string | undefined,
  empty: { headline: string; detail?: string },
): ScreenState<readonly T[]> {
  if (error !== undefined) {
    return { kind: 'error', headline: 'That did not work', detail: error }
  }

  if (items === undefined) {
    return { kind: 'loading' }
  }
  if (items.length === 0) {
    // Spread rather than always setting `detail`: this project turns on
    // `exactOptionalPropertyTypes`, where an explicit `undefined` and an absent
    // key are different things — which is the point of the setting.
    return { kind: 'empty', headline: empty.headline, ...detail(empty.detail) }
  }
  return { kind: 'content', value: items }
}

/** An optional detail, present only when there is one. */
function detail(text: string | undefined): { detail?: string } {
  return text === undefined ? {} : { detail: text }
}

/** Make an element with a class and optional text. */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag)
  if (className !== undefined) {
    node.className = className
  }
  if (text !== undefined) {
    node.textContent = text
  }
  return node
}

/** The app bar: a back affordance when there is somewhere to go, and a title. */
export function appBar(options: {
  title: string
  subtitle?: string
  onBack?: () => void
  /**
   * Makes the title itself the way to switch instances.
   *
   * A separate button would need a third slot in a bar that already has two on
   * a 390px screen. The title is also the honest control: it is the name of the
   * thing being switched.
   */
  onTitleTap?: () => void
  /** The mark to wear, where a screen has one. */
  mark?: import('./mark.js').MarkKind
  action?: { label: string; onTap: () => void }
}): HTMLElement {
  const bar = el('header', 'bar')

  if (options.onBack) {
    const back = el('button', 'icon-button', '‹')
    // Named, because "‹" is read aloud as nothing useful.
    back.setAttribute('aria-label', 'Back')
    back.addEventListener('click', options.onBack)
    bar.append(back)
  }

  if (options.mark) {
    bar.append(markElement(options.mark))
  }

  const heading = el('h1')
  if (options.onTitleTap) {
    // A real button, not an h1 with a click handler: this has to be reachable
    // by keyboard and announced as a control, and a heading is neither.
    const tap = el('button', 'title-button')
    tap.setAttribute('aria-label', `${options.title} — switch instance`)
    tap.append(document.createTextNode(options.title), el('span', 'chevron-down', '⌄'))
    heading.append(tap)
    tap.addEventListener('click', options.onTitleTap)
  } else {
    heading.append(document.createTextNode(options.title))
  }
  if (options.subtitle !== undefined) {
    heading.append(el('span', 'subtitle', options.subtitle))
  }
  bar.append(heading)

  if (options.action) {
    const action = el('button', 'icon-button', '+')
    action.setAttribute('aria-label', options.action.label)
    action.addEventListener('click', options.action.onTap)
    bar.append(action)
  }
  return bar
}

/** A row in a list: a status dot, a label, and a chevron if it leads somewhere. */
export function row(options: {
  title: string
  meta?: string
  tone?: 'needs-you' | 'working' | 'ok'
  /** What a screen reader says. Never only the colour. */
  label: string
  leadsSomewhere?: boolean
  onTap?: () => void
}): HTMLElement {
  const button = el('button', 'tap')
  button.setAttribute('aria-label', options.label)

  const dot = el('span', options.tone ? `dot ${options.tone}` : 'dot')
  dot.setAttribute('aria-hidden', 'true')
  button.append(dot)

  const label = el('span', 'label')
  label.append(el('span', 'title', options.title))
  if (options.meta !== undefined) {
    label.append(el('span', 'meta', options.meta))
  }
  button.append(label)

  if (options.leadsSomewhere !== false) {
    const chevron = el('span', 'chevron', '›')
    chevron.setAttribute('aria-hidden', 'true')
    button.append(chevron)
  }

  if (options.onTap) {
    button.addEventListener('click', options.onTap)
  } else {
    button.disabled = true
  }
  return button
}

/** Render whichever single state a screen is in. */
export function renderState<T>(
  state: ScreenState<T>,
  content: (value: T) => HTMLElement | HTMLElement[],
): HTMLElement {
  switch (state.kind) {
    case 'loading': {
      const box = el('div', 'state')
      const spinner = el('div', 'spinner')
      spinner.setAttribute('role', 'status')
      spinner.setAttribute('aria-label', 'Loading')
      box.append(spinner)
      return box
    }
    case 'empty':
    case 'error': {
      const box = el('div', state.kind === 'error' ? 'state error' : 'state')
      box.append(el('p', 'headline', state.headline))
      if (state.detail !== undefined) {
        box.append(el('p', 'detail', state.detail))
      }
      return box
    }
    case 'content': {
      const box = el('div')
      const rendered = content(state.value)
      box.append(...(Array.isArray(rendered) ? rendered : [rendered]))
      return box
    }
  }
}

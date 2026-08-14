/**
 * Where the client is, and how it got there.
 *
 * A stack over the browser's own history. The phone's back gesture is the one
 * navigation control every user already has, and a web app that ignores it
 * throws them out to the previous website instead of the previous screen.
 */

/** A screen, and what it needs to draw itself. */
export type Route =
  | { screen: 'roster' }
  | { screen: 'terminal'; session: string }
  | { screen: 'threads'; session: string }
  | { screen: 'card'; interaction: string }
  | { screen: 'instances' }
  | { screen: 'blocks'; session: string }
  | { screen: 'transcript'; session: string }
  | { screen: 'search'; session: string }
  // The surfaces this client's own documents specify and had no route to.
  // Added to the union rather than reached ad hoc, because TypeScript's
  // exhaustiveness is what stops a new screen shipping with no title, no back
  // destination and no place in the deep-link table.
  | { screen: 'projects' }
  | { screen: 'files'; workspace: string; path: string }
  | { screen: 'changes'; workspace: string }
  | { screen: 'diff'; workspace: string; path: string }
  | { screen: 'settings' }

/** What a route is called, for the bar. */
export function titleOf(route: Route, context: { title?: string } = {}): string {
  switch (route.screen) {
    case 'roster':
      return 'omt'
    case 'terminal':
      return context.title ?? 'Terminal'
    case 'threads':
      return context.title ?? 'Agents'
    case 'card':
      return 'Needs you'
    case 'instances':
      return 'Instances'
    case 'blocks':
      return context.title ?? 'Commands'
    case 'transcript':
      return context.title ?? 'Conversation'
    case 'search':
      return 'Search'
    case 'projects':
      return 'Open a project'
    case 'files':
      // The directory being looked at, not the workspace: the workspace is
      // named on the screen above and repeating it here costs the one thing a
      // title has, which is the answer to "where am I now".
      return route.path === '' ? (context.title ?? 'Files') : basename(route.path)
    case 'changes':
      return 'Changes'
    case 'diff':
      return basename(route.path)
    case 'settings':
      return 'Settings'
  }
}

/** The last segment of a workspace-relative path. */
function basename(path: string): string {
  const last = path.split('/').filter(Boolean).pop()
  return last ?? path
}

/** Whether this screen has something to go back to. */
export function hasBack(route: Route): boolean {
  return route.screen !== 'roster'
}

/**
 * The screen stack.
 *
 * Deliberately not a list of every screen ever visited: `history` already is
 * that, and keeping a second one would let the two disagree about where back
 * goes. This holds the current route and mirrors it into history.
 */
export class Navigator {
  #route: Route = { screen: 'roster' }
  #listeners = new Set<(route: Route) => void>()

  constructor(private readonly history: History = window.history) {}

  /** Where we are. */
  get route(): Route {
    return this.#route
  }

  /** Be told when it changes. */
  subscribe(listener: (route: Route) => void): () => void {
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }

  /** Go somewhere, and let the back gesture return. */
  push(route: Route): void {
    this.#route = route
    this.history.pushState({ route }, '')
    this.#emit()
  }

  /**
   * Replace where we are without adding a step.
   *
   * For a redirect — landing on the roster and immediately opening the only
   * session — where a pushed entry would make back appear to do nothing.
   */
  replace(route: Route): void {
    this.#route = route
    this.history.replaceState({ route }, '')
    this.#emit()
  }

  /** Go back, if there is anywhere to go. */
  back(): void {
    this.history.back()
  }

  /**
   * Apply a route the browser restored.
   *
   * Called from `popstate`. A state without a route is the entry the page
   * loaded on, which is the roster.
   */
  restore(state: unknown): void {
    const route =
      state !== null && typeof state === 'object' && 'route' in state
        ? (state as { route: Route }).route
        : ({ screen: 'roster' } as Route)
    this.#route = route
    this.#emit()
  }

  #emit(): void {
    for (const listener of this.#listeners) {
      listener(this.#route)
    }
  }
}

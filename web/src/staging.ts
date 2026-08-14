/**
 * What is on its way to the instance, and what happened to it.
 *
 * Separate from the composer because the rule it enforces is not about
 * rendering: **a failure names the attachment that failed and never costs the
 * text somebody typed.**
 *
 * On a phone the typed message is the expensive thing on the screen — it took
 * a soft keyboard and both thumbs — and a client that clears it because an
 * upload failed has thrown away the wrong half. It is also why an attachment
 * appears the instant it is chosen rather than when it lands: the gap between
 * tapping and anything happening is where somebody taps again, and the second
 * tap sends the file twice.
 */

/** One attachment, and where it has got to. */
export interface Staged {
  /** A key of this client's own, because two files can share a name. */
  readonly id: number
  readonly name: string
  readonly state: 'sending' | 'failed'
  /** Why, when it failed. Shown, because "something failed" is not actionable
   *  when three things are in flight. */
  readonly reason?: string
}

/**
 * The staging area.
 *
 * Deliberately holds no text. The composer owns what was typed and this owns
 * what is being sent, so there is no code path on which one can clear the
 * other — which is the whole point, rather than a discipline somebody has to
 * remember.
 */
export class Attachments {
  #items: Staged[] = []
  #next = 1

  /** Note that one has started. Returns its id, for the calls that follow. */
  begin(name: string): number {
    const id = this.#next
    this.#next += 1
    this.#items.push({ id, name, state: 'sending' })
    return id
  }

  /** It arrived. Removed rather than marked done: an attachment that landed is
   *  in the conversation now, and a list of things that already worked is a
   *  list nobody reads. */
  succeed(id: number): void {
    this.#items = this.#items.filter((item) => item.id !== id)
  }

  /** It did not. Kept, and named, until somebody deals with it. */
  fail(id: number, reason: string): void {
    this.#items = this.#items.map((item) =>
      item.id === id ? { ...item, state: 'failed' as const, reason } : item,
    )
  }

  /** Stop showing a failure somebody has read. */
  dismiss(id: number): void {
    this.#items = this.#items.filter((item) => item.id !== id)
  }

  /** Everything worth showing, oldest first. */
  list(): readonly Staged[] {
    return this.#items
  }

  /** Whether anything is still in flight. */
  busy(): boolean {
    return this.#items.some((item) => item.state === 'sending')
  }
}

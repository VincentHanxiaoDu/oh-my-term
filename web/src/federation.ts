/**
 * One list across every instance, rather than a per-instance drill-down.
 *
 * `docs/architecture/08-web-client.md` §3.3. The reason is the use it is put
 * to: somebody has a laptop and a desktop, both running agents, and the
 * question they pick the phone up to answer is "does anything need me" — not
 * "does anything on the laptop need me".
 *
 * A per-instance list makes them ask it twice and remember which one they
 * already checked.
 */

/** One session, wherever it lives. */
export interface UnifiedSession {
  /** Which instance it is on. */
  instance: string
  /** What that instance is called, for the row. */
  instanceLabel: string
  id: string
  title: string
  state: string
  /** Which workspace it belongs to, so a row can say where it is.
   *
   * Carried through the merge rather than looked up afterwards: a roster is
   * mostly sessions called "shell", and without this every row on the screen
   * is identical and picking one is guessing. */
  workspace?: string
  /** Its live working directory, as a fallback for where. */
  cwd?: string
  /** How many agent threads are in it. */
  threads: number
  /** Whether that instance is reachable at all. */
  reachable: boolean
}

/** What one instance contributes. */
export interface InstanceRoster {
  instance: string
  label: string
  reachable: boolean
  sessions: { id: string; title: string; state: string; workspace?: string; cwd?: string }[]
  threads?: Record<string, number>
}

/** Whether a state means the agent or shell is actually doing something. */
function isRunning(state: string): boolean {
  return state === 'running' || state === 'working' || state === 'starting'
}

/** Whether a state means somebody has to do something. */
function needsSomebody(state: string): boolean {
  return state === 'blocked'
}

/**
 * The line under the title: the answer, then the count.
 *
 * Here rather than beside either caller because there were two of them saying
 * different things. The roster used one phrasing for a single instance and
 * this file used another for several, so the headline changed wording the
 * moment somebody paired a second machine — and the one people actually read
 * was whichever this file happened to say.
 *
 * The question is "does anything need me", so the line answers that. A count
 * on its own — "4 sessions" — is the single fact nobody unlocks their phone
 * to learn.
 */
export function waitingLine(total: number, blocked: number, running = 0): string {
  if (blocked > 0) {
    return blocked === 1 ? '1 waiting on you' : `${blocked} waiting on you`
  }
  if (total === 0) {
    return 'nothing running'
  }
  // "Running" has to mean running. Counting every session as running said
  // "7 running" over a list where all seven read "orphaned" — a headline
  // contradicted by the rows underneath it, which is the exact failure this
  // line was rewritten to fix.
  if (running > 0) {
    return `nothing waiting \u00b7 ${running === 1 ? '1 running' : `${running} running`}`
  }
  // Open rather than idle: an orphaned session is not idle, it is a session
  // whose process is gone and whose output is still there. One word that is
  // true of every state left beats a word that is true of most of them.
  return `nothing waiting \u00b7 ${total === 1 ? '1 open' : `${total} open`}`
}

/**
 * Merge every instance's sessions into one ordered list.
 *
 * The order is the whole point: what needs a human, then what is working, then
 * everything else, and only *within* those by name so the list does not shuffle
 * between glances. Never grouped by instance — grouping by instance is the
 * per-instance drill-down again, with extra scrolling.
 */
export function unify(rosters: readonly InstanceRoster[]): UnifiedSession[] {
  const rows: UnifiedSession[] = []
  for (const roster of rosters) {
    for (const session of roster.sessions) {
      rows.push({
        instance: roster.instance,
        instanceLabel: roster.label,
        id: session.id,
        title: session.title,
        state: session.state,
        ...(session.workspace === undefined ? {} : { workspace: session.workspace }),
        ...(session.cwd === undefined ? {} : { cwd: session.cwd }),
        threads: roster.threads?.[session.id] ?? 0,
        reachable: roster.reachable,
      })
    }
  }

  const rank = (row: UnifiedSession) =>
    needsSomebody(row.state) ? 0 : row.state === 'working' || row.state === 'running' ? 1 : 2

  return rows.sort(
    (a, b) =>
      rank(a) - rank(b) ||
      a.title.localeCompare(b.title) ||
      a.instanceLabel.localeCompare(b.instanceLabel),
  )
}

/**
 * What the roster's subtitle says across every instance.
 *
 * Counted from the same rows the body renders — the bug that started the whole
 * rebuild was a header counting one thing and a body counting another, and
 * adding instances is exactly where that would happen again.
 */
export function unifiedSubtitle(rows: readonly UnifiedSession[], instances: number): string {
  const blocked = rows.filter((r) => needsSomebody(r.state)).length
  const unreachable = new Set(rows.filter((r) => !r.reachable).map((r) => r.instance)).size

  const head = waitingLine(
    rows.length,
    blocked,
    rows.filter((r) => isRunning(r.state)).length,
  )

  if (unreachable > 0) {
    // Said, because a roster missing a machine's sessions looks exactly like a
    // machine with nothing running on it.
    return `${head} · ${unreachable} instance${unreachable === 1 ? '' : 's'} unreachable`
  }
  return instances > 1 ? `${head} · ${instances} instances` : head
}

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

/**
 * Whether a session's process is gone.
 *
 * An orphan is a session omt restored from a snapshot: the scrollback, the
 * cwd and the argv came back, and the pty did not. It is readable and
 * searchable and refuses writes. It is not idle, not running, and not an
 * error, so it is counted as itself.
 */
export function isOrphaned(state: string): boolean {
  return state === 'orphaned'
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
export function waitingLine(
  total: number,
  blocked: number,
  running = 0,
  orphaned = 0,
): string {
  if (blocked > 0) {
    return blocked === 1 ? '1 waiting on you' : `${blocked} waiting on you`
  }
  if (total === 0) {
    return 'nothing running'
  }
  // Every session is a corpse. Saying "16 open" over sixteen rows that all
  // read "orphaned" is the same failure as saying "7 running" over them: a
  // headline the rows underneath contradict. This is what somebody comes back
  // to after a reboot, and it is worth one plain sentence.
  if (running === 0 && orphaned === total) {
    // Short, because this is an app bar on a phone: the long version of this
    // sentence — "3 sessions ended with omt" — was truncated to "3 sessions
    // end\u2026", a headline that stops mid-word. The rows underneath say
    // "orphaned"; this line only has to account for the count.
    return `nothing running \u00b7 ${orphaned} ended`
  }
  // "Running" has to mean running. Counting every session as running said
  // "7 running" over a list where all seven read "orphaned" — a headline
  // contradicted by the rows underneath it, which is the exact failure this
  // line was rewritten to fix.
  if (running > 0) {
    const live = `nothing waiting \u00b7 ${running === 1 ? '1 running' : `${running} running`}`
    // Two numbers, not one. Folding orphans into the running count is the bug
    // this line already had once; folding them into nothing at all leaves a
    // roster whose length nobody can account for.
    return orphaned > 0 ? `${live} \u00b7 ${orphaned} ended` : live
  }
  // Open rather than idle: an orphaned session is not idle, it is a session
  // whose process is gone and whose output is still there. One word that is
  // true of every state left beats a word that is true of most of them.
  return `nothing waiting \u00b7 ${total === 1 ? '1 open' : `${total} open`}`
}

/**
 * What to call a session's state to somebody reading a list.
 *
 * The same words the TUI uses, from the same rules, because a person looking
 * at one surface and then the other must not have to work out that "blocked"
 * and "waiting on you" are the same thing. The wire carries the state; what to
 * call it was a decision each surface was making separately, and differently.
 *
 * `seen` is not on the wire and is not the agent's business: an agent is idle
 * either way, and whether *you* have looked at it is a fact about you. An
 * agent that finished while you were elsewhere is the one thing this product
 * exists to surface, and calling that "idle" is how somebody misses it.
 */
export function stateWord(state: string, seen = true): string {
  switch (state) {
    case 'blocked':
      return 'waiting on you'
    case 'working':
    case 'running':
      return 'working'
    case 'starting':
      return 'starting'
    case 'orphaned':
      return 'ended with omt'
    case 'exited':
      return 'exited'
    case 'idle':
      return seen ? 'idle' : 'done'
    case 'unknown':
      // Never the word itself. It describes what omt knows rather than what
      // the agent is, and beside a session omt has just listed it reads as
      // "omt has no idea what this is".
      return 'state not reported yet'
    default:
      // A state from a newer instance is a word this client does not have.
      // Printing it is honest; guessing at a friendlier one is not.
      return state
  }
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
    rows.filter((r) => isOrphaned(r.state)).length,
  )

  if (unreachable > 0) {
    // Said, because a roster missing a machine's sessions looks exactly like a
    // machine with nothing running on it.
    return `${head} · ${unreachable} instance${unreachable === 1 ? '' : 's'} unreachable`
  }
  return instances > 1 ? `${head} · ${instances} instances` : head
}

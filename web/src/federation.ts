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
  sessions: { id: string; title: string; state: string }[]
  threads?: Record<string, number>
}

/** Whether a state means somebody has to do something. */
function needsSomebody(state: string): boolean {
  return state === 'blocked'
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

  const head =
    blocked > 0
      ? `${blocked} of ${rows.length} need you`
      : rows.length === 1
        ? '1 session'
        : `${rows.length} sessions`

  if (unreachable > 0) {
    // Said, because a roster missing a machine's sessions looks exactly like a
    // machine with nothing running on it.
    return `${head} · ${unreachable} instance${unreachable === 1 ? '' : 's'} unreachable`
  }
  return instances > 1 ? `${head} · ${instances} instances` : head
}

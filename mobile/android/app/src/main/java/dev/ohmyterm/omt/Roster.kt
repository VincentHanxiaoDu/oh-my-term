package dev.ohmyterm.omt

/**
 * A session, as the roster needs it.
 */
data class SessionRow(val id: String, val title: String, val state: String) {
    /** Whether this row is the reason the phone buzzed. */
    val needsYou: Boolean get() = state == "blocked"
}

/**
 * Order the roster.
 *
 * Blocked first, then working, then idle — and never by spawn order, which
 * buries the one row that matters behind four that do not. Ties break by name
 * so the list does not shuffle between glances.
 */
fun orderRoster(rows: List<SessionRow>): List<SessionRow> {
    fun rank(state: String) = when (state) {
        "blocked" -> 0
        "working" -> 1
        "idle" -> 2
        else -> 3
    }
    return rows.sortedWith(compareBy({ rank(it.state) }, { it.title }))
}

/**
 * The one line above the list.
 *
 * Leads with the count that decides whether to keep reading, and says why it is
 * empty rather than showing an empty list and leaving somebody to guess.
 */
fun rosterHeader(rows: List<SessionRow>, connection: String, refusal: String?): String {
    if (connection != "connected") return refusal ?: "$connection…"
    val blocked = rows.count { it.needsYou }
    if (blocked > 0) return "$blocked of ${rows.size} need you"
    return if (rows.isEmpty()) "no sessions" else "${rows.size} running"
}

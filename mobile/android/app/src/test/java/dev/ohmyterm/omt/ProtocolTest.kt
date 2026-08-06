package dev.ohmyterm.omt

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProtocolTest {
    @Test
    fun commandsNeedAnIntentAndQueriesDoNot() {
        assertTrue(Protocol.isCommand("session.write"))
        assertFalse(Protocol.isCommand("session.read"))
    }

    @Test
    fun theTokenTravelsInTheSubprotocolBecauseASocketHasNoHeaders() {
        val offered = Protocol.subprotocols("tok")
        assertEquals(listOf("omt.v1", "omt.token.tok"), offered)
    }
}

class RosterTest {
    @Test
    fun whatNeedsAHumanComesFirst() {
        // Spawn order buries the one row that matters behind four that do not.
        val ordered = orderRoster(
            listOf(
                SessionRow("1", "aaa", "idle"),
                SessionRow("2", "zzz", "blocked"),
                SessionRow("3", "mmm", "working"),
            )
        )
        assertEquals("zzz", ordered.first().title)
        assertEquals("aaa", ordered.last().title)
    }

    @Test
    fun tiesBreakByNameSoTheListIsStable() {
        val ordered = orderRoster(
            listOf(SessionRow("1", "beta", "working"), SessionRow("2", "alpha", "working"))
        )
        assertEquals(listOf("alpha", "beta"), ordered.map { it.title })
    }

    @Test
    fun theHeaderLeadsWithTheCountThatDecidesWhetherToKeepReading() {
        val rows = listOf(SessionRow("1", "a", "blocked"), SessionRow("2", "b", "idle"))
        assertEquals("1 of 2 need you", rosterHeader(rows, "connected", null))
    }

    @Test
    fun aRefusalIsSaidOutLoudRatherThanShownAsAnEmptyList() {
        assertEquals(
            "that token is not valid",
            rosterHeader(emptyList(), "refused", "that token is not valid"),
        )
    }
}

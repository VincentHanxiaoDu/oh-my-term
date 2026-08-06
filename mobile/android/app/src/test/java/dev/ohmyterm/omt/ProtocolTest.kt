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

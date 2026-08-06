package dev.ohmyterm.omt

/**
 * The wire, as the browser client already speaks it.
 *
 * Hand-written here and generated for the browser — a difference worth removing
 * rather than living with. The capability list belongs in `cargo xtask codegen`
 * beside the TypeScript one, committed and diffed in CI, because a hand-written
 * list is how a client comes to believe in a capability the server does not
 * offer, and the symptom is a button that does nothing.
 */
object Protocol {
    /**
     * Whether a capability changes anything.
     *
     * A command needs an intent id and a query must not carry one. The daemon
     * refuses a command without it, which is what makes a retry after a dropped
     * acknowledgement recognisable rather than a second execution.
     */
    private val COMMANDS = setOf(
        "workspace.open", "session.create", "session.close", "session.write",
        "session.resize", "session.acquire", "session.release", "interaction.respond",
        "agent.interrupt", "fs.write", "pane.open", "pane.close", "pane.focus",
    )

    fun isCommand(capability: String): Boolean = capability in COMMANDS

    /**
     * The subprotocols to offer on the socket.
     *
     * The token travels here because a WebSocket cannot carry a header. A query
     * string would land in access logs, browser history and any Referer — and
     * on Android, in whatever proxy the network operator runs.
     */
    fun subprotocols(token: String): List<String> = listOf("omt.v1", "omt.token.$token")
}

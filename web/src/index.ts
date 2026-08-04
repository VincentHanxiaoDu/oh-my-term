/**
 * The omt web client.
 *
 * Two things this package exists to get right, because both fail silently.
 *
 * A client's position advances only for events it actually delivered, so a
 * reconnect cannot resume past something that was dropped. And whether a card
 * can be answered is read from its `deliverable`, never from its state — an
 * open card is not necessarily one omt can answer, and a button offered for one
 * it cannot means the user finds out by the wrong option being chosen.
 */

export * from './protocol.js'
export * from './resume.js'
export * from './session.js'
export * from './threads.js'
export * from './capabilities.js'
export * from './connect.js'
export * from './store.js'
export * from './screen.js'
export * from './touch.js'
export * from './generated/catalog.js'

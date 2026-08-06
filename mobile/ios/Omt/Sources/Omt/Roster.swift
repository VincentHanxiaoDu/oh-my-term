// The roster, which is the argument of the whole client.
//
// A phone is picked up for ninety seconds because something buzzed. Opening to
// a wall of terminal output has already failed that, so the first screen is the
// list — and the row that needs a human is at the top of it, whatever it is
// called and whenever it started.

import Foundation

/// A session, as the roster needs it.
public struct SessionRow: Identifiable, Equatable, Sendable {
    public let id: String
    public let title: String
    public let state: String

    public init(id: String, title: String, state: String) {
        self.id = id
        self.title = title
        self.state = state
    }

    /// Whether this row is the reason the phone buzzed.
    public var needsYou: Bool { state == "blocked" }
}

/// Order the roster.
///
/// Blocked first, then working, then idle — and never by spawn order, which
/// buries the one row that matters behind four that do not. Ties break by name
/// so the list does not shuffle between glances.
public func orderRoster(_ rows: [SessionRow]) -> [SessionRow] {
    func rank(_ state: String) -> Int {
        switch state {
        case "blocked": return 0
        case "working": return 1
        case "idle": return 2
        default: return 3
        }
    }
    return rows.sorted {
        rank($0.state) == rank($1.state)
            ? $0.title < $1.title
            : rank($0.state) < rank($1.state)
    }
}

/// The one line above the list.
///
/// Leads with the count that decides whether to keep reading, and says why it
/// is empty rather than showing an empty list and leaving somebody to guess.
public func rosterHeader(rows: [SessionRow], connection: String, refusal: String?) -> String {
    guard connection == "connected" else { return refusal ?? "\(connection)…" }
    let blocked = rows.filter(\.needsYou).count
    if blocked > 0 { return "\(blocked) of \(rows.count) need you" }
    return rows.isEmpty ? "no sessions" : "\(rows.count) running"
}

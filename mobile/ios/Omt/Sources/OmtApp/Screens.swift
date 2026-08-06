// The screens.
//
// The roster first, because a phone is picked up for ninety seconds because
// something buzzed — and opening to a wall of terminal output has already
// failed that.

import SwiftUI
import OmtClient

/// The list, with what needs a human at the top of it.
public struct RosterView: View {
    @ObservedObject var client: Client

    public init(client: Client) {
        self.client = client
    }

    public var body: some View {
        NavigationStack {
            List(client.rows) { row in
                let description = row.needsYou ? "needs you" : row.state
                HStack {
                    // A shape as well as a colour. Colour alone is invisible to
                    // a tenth of the people who will use this.
                    Image(systemName: row.needsYou ? "exclamationmark.circle.fill" : "circle")
                        .foregroundStyle(row.needsYou ? .orange : .secondary)
                    Text(row.title)
                    Spacer()
                    Text(row.state)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                // Said in words, because a screen reader gets nothing from a
                // colour or a glyph.
                .accessibilityLabel("\(row.title), \(description)")
            }
            .navigationTitle(client.header)
            .refreshable { client.refresh() }
            .overlay {
                if client.rows.isEmpty {
                    // Says why it is empty rather than showing an empty list
                    // and leaving somebody to guess.
                    ContentUnavailableView(
                        client.header,
                        systemImage: "terminal",
                        description: Text("Nothing is running, or nothing has answered yet.")
                    )
                }
            }
        }
    }
}

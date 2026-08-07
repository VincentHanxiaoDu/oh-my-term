// The socket, and the state a screen draws from.
//
// One connection held by one object, because two would mean two answers to
// "what is running" and the phone would show whichever arrived last.

import Foundation
#if canImport(OmtClient)
import OmtClient
#endif

/// Everything the app knows, and the only thing that changes it.
@MainActor
public final class Client: ObservableObject {
    @Published public private(set) var rows: [SessionRow] = []
    @Published public private(set) var connection = "disconnected"
    @Published public private(set) var refusal: String?

    private var socket: URLSessionWebSocketTask?
    private let device: String
    private var next: UInt64 = 0

    public init(defaults: UserDefaults = .standard) {
        // Stable across launches. A fresh id every start makes every reconnect
        // look like a second person answering the same card.
        let key = "omt.device"
        if let existing = defaults.string(forKey: key) {
            device = existing
        } else {
            let fresh = UUID().uuidString
            defaults.set(fresh, forKey: key)
            device = fresh
        }
    }

    /// What the roster's header says.
    public var header: String {
        rosterHeader(rows: rows, connection: connection, refusal: refusal)
    }

    /// Connect to an instance.
    ///
    /// The token travels in the subprotocol because a WebSocket cannot carry a
    /// header. A query string would land in access logs and in whatever proxy
    /// the network operator runs, and a token in either outlives whoever
    /// pasted it.
    public func connect(to url: URL, token: String) {
        connection = "connecting"
        var request = URLRequest(url: url.appendingPathComponent("api/ws"))
        request.setValue(
            "omt.v1, omt.token.\(token)",
            forHTTPHeaderField: "Sec-WebSocket-Protocol"
        )
        let task = URLSession.shared.webSocketTask(with: request)
        socket = task
        task.resume()
        listen()
        send(["t": "hello", "proto": 1, "client": "omt-ios"])
    }

    /// Ask what is running.
    public func refresh() {
        call("session.list", input: [:])
    }

    /// Build one call, exactly as the browser client builds it.
    ///
    /// Exposed for the checks: what this produces is the thing that has to be
    /// right, and it is right or wrong without a socket.
    public func message(for capability: String, input: [String: Any]) -> [String: Any] {
        var message: [String: Any] = [
            "t": "call",
            "request": ["device": device, "n": next + 1],
            "capability": capability,
            "input": input,
        ]
        // A command needs an intent id and a query must not carry one. The
        // daemon refuses a command without it, which is what makes a retry
        // after a dropped acknowledgement recognisable rather than a second
        // execution.
        if isCommand(capability) {
            message["intent"] = UUID().uuidString.lowercased()
        }
        return message
    }

    private func call(_ capability: String, input: [String: Any]) {
        next += 1
        send(message(for: capability, input: input))
    }

    private func send(_ message: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: message),
              let text = String(data: data, encoding: .utf8)
        else { return }
        socket?.send(.string(text)) { _ in }
    }

    private func listen() {
        socket?.receive { [weak self] result in
            Task { @MainActor in
                guard let self else { return }
                switch result {
                case let .success(.string(text)):
                    self.apply(text)
                    self.listen()
                case .success:
                    self.listen()
                case let .failure(error):
                    // Said out loud rather than retried silently: a refused
                    // credential will not fix itself, and a spinner forever is
                    // the least informative possible outcome.
                    self.connection = "refused"
                    self.refusal = error.localizedDescription
                }
            }
        }
    }

    /// Apply one message from the instance.
    ///
    /// Public so the checks can drive it with a recorded message. What a
    /// message means does not need a network to be tested.
    public func apply(_ text: String) {
        guard let data = text.data(using: .utf8),
              let value = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return }

        if value["t"] as? String == "welcome" {
            connection = "connected"
            // The welcome says what the instance offers, not what it holds.
            // Without asking, the roster sits empty and looks exactly like an
            // instance with nothing running.
            refresh()
            return
        }
        if let output = value["output"] as? [String: Any],
           let sessions = output["sessions"] as? [[String: Any]] {
            rows = orderRoster(sessions.compactMap { s in
                guard let id = s["id"] as? String else { return nil }
                return SessionRow(
                    id: id,
                    title: s["title"] as? String ?? id,
                    state: s["state"] as? String ?? "unknown"
                )
            })
        }
    }
}

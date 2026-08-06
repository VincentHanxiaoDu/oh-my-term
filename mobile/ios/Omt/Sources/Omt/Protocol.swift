// The wire, as the browser client already speaks it.
//
// Written by hand here and generated in the browser, which is a difference
// worth removing rather than living with: the capability list belongs in
// `cargo xtask codegen` alongside the TypeScript one, committed and diffed in
// CI. Writing it by hand is how a client comes to believe in a capability the
// server does not offer, and the symptom is a button that does nothing.

import Foundation

/// A request omt can recognise across a reconnection.
public struct RequestId: Codable, Equatable, Sendable {
    /// This device, stable across launches — a fresh one on every start makes
    /// every reconnect look like a second person answering.
    public let device: String
    /// Monotonic within the device.
    public let n: UInt64

    public init(device: String, n: UInt64) {
        self.device = device
        self.n = n
    }
}

/// What a client sends.
public enum ClientMessage: Encodable, Sendable {
    case hello(proto: Int, client: String, token: String?)
    case call(request: RequestId, capability: String, input: [String: AnyCodable], intent: String?)

    enum CodingKeys: String, CodingKey {
        case t, proto, client, token, request, capability, input, intent
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case let .hello(proto, client, token):
            try c.encode("hello", forKey: .t)
            try c.encode(proto, forKey: .proto)
            try c.encode(client, forKey: .client)
            try c.encodeIfPresent(token, forKey: .token)
        case let .call(request, capability, input, intent):
            try c.encode("call", forKey: .t)
            try c.encode(request, forKey: .request)
            try c.encode(capability, forKey: .capability)
            try c.encode(input, forKey: .input)
            // Present on commands and absent on queries. The daemon refuses a
            // command without one, which is what makes a retry after a dropped
            // acknowledgement recognisable rather than a second execution.
            try c.encodeIfPresent(intent, forKey: .intent)
        }
    }
}

/// Whether a capability changes anything, which decides whether it needs an
/// intent id. Derived from the catalog rather than listed here once this is
/// generated — see the note at the top of this file.
public func isCommand(_ capability: String) -> Bool {
    let commands: Set<String> = [
        "workspace.open", "session.create", "session.close", "session.write",
        "session.resize", "session.acquire", "session.release", "interaction.respond",
        "agent.interrupt", "fs.write", "pane.open", "pane.close", "pane.focus",
    ]
    return commands.contains(capability)
}

/// A JSON value, because the protocol's inputs are open-ended.
public struct AnyCodable: Encodable, Sendable {
    private let encodeTo: @Sendable (Encoder) throws -> Void

    public init(_ value: String) { encodeTo = { try value.encode(to: $0) } }
    public init(_ value: Int) { encodeTo = { try value.encode(to: $0) } }
    public init(_ value: Bool) { encodeTo = { try value.encode(to: $0) } }

    public func encode(to encoder: Encoder) throws { try encodeTo(encoder) }
}

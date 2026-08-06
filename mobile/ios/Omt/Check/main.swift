// The client's own assertions, as a program.
//
// A test target would need XCTest, which needs a full Xcode. This runs with the
// command line tools, which means it runs in CI and on a fresh clone — and a
// check that only runs on one machine is a check nobody runs.

import Foundation
import OmtClient

var failures: [String] = []

func check(_ condition: Bool, _ what: String) {
    if !condition { failures.append(what) }
}

// A command without an intent id is refused by the daemon outright, so a client
// that omits one can read everything and change nothing.
check(isCommand("session.write"), "session.write is a command")
check(!isCommand("session.read"), "session.read is a query")

let command = ClientMessage.call(
    request: RequestId(device: "d", n: 1),
    capability: "session.close",
    input: ["session": AnyCodable("s1")],
    intent: "11111111-1111-1111-1111-111111111111"
)
let commandText = String(decoding: try JSONEncoder().encode(command), as: UTF8.self)
check(commandText.contains("\"intent\""), "a command carries its intent id")

// And a query must not: an id on a read suggests the server should remember it.
let query = ClientMessage.call(
    request: RequestId(device: "d", n: 1),
    capability: "session.read",
    input: [:],
    intent: nil
)
let queryText = String(decoding: try JSONEncoder().encode(query), as: UTF8.self)
check(!queryText.contains("intent"), "a query carries no intent id")

if failures.isEmpty {
    print("omt swift client: \(4) checks passed")
} else {
    for f in failures { print("FAILED: \(f)") }
    exit(1)
}

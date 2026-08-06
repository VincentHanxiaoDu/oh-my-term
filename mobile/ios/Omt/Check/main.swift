// The client's own assertions, as a program.
//
// A test target would need XCTest, which needs a full Xcode. This runs with the
// command line tools, which means it runs in CI and on a fresh clone — and a
// check that only runs on one machine is a check nobody runs.

import Foundation
import OmtClient
import OmtApp

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

// The roster's ordering, which is the client's whole argument: what needs a
// human is at the top, whatever it is called and whenever it started.
let ordered = orderRoster([
    SessionRow(id: "1", title: "aaa", state: "idle"),
    SessionRow(id: "2", title: "zzz", state: "blocked"),
    SessionRow(id: "3", title: "mmm", state: "working"),
])
check(ordered.first?.title == "zzz", "what needs a human comes first")
check(ordered.last?.title == "aaa", "idle sinks to the bottom")

let sorted = orderRoster([
    SessionRow(id: "1", title: "beta", state: "working"),
    SessionRow(id: "2", title: "alpha", state: "working"),
])
check(sorted.map(\.title) == ["alpha", "beta"], "ties break by name, so the list is stable")

check(
    rosterHeader(rows: ordered, connection: "connected", refusal: nil) == "1 of 3 need you",
    "the header leads with the count that decides whether to keep reading"
)
check(
    rosterHeader(rows: [], connection: "refused", refusal: "that token is not valid")
        == "that token is not valid",
    "a refusal is said out loud rather than shown as an empty list"
)

// The app's own message building and message handling, driven without a
// socket. What a message means does not need a network to be tested.
let defaults = UserDefaults(suiteName: "omt.check") ?? .standard
defaults.removePersistentDomain(forName: "omt.check")
let client = await Client(defaults: defaults)

let appCommand = await client.message(for: "session.close", input: ["session": "s1"])
check(appCommand["intent"] != nil, "the app puts an intent id on a command")
let appQuery = await client.message(for: "session.list", input: [:])
check(appQuery["intent"] == nil, "the app leaves a query without one")

// A device id that survives a launch. A fresh one every start makes every
// reconnect look like a second person answering the same card.
let first = await client.message(for: "session.list", input: [:])["request"] as? [String: Any]
let again = await Client(defaults: defaults)
let second = await again.message(for: "session.list", input: [:])["request"] as? [String: Any]
check(
    (first?["device"] as? String) == (second?["device"] as? String),
    "the device id survives a relaunch"
)

await client.apply(#"{"t":"welcome","proto":1,"role":"operator","capabilities":[]}"#)
let connected = await client.connection
check(connected == "connected", "a welcome connects the client")

await client.apply(
    #"{"t":"result","output":{"sessions":[{"id":"1","title":"aaa","state":"idle"},{"id":"2","title":"zzz","state":"blocked"}]}}"#
)
let listed = await client.rows
check(listed.first?.title == "zzz", "the app orders what needs a human first")
let liveHeader = await client.header
check(liveHeader == "1 of 2 need you", "the header counts what needs you")

if failures.isEmpty {
    print("omt swift client: \(15) checks passed")
} else {
    for f in failures { print("FAILED: \(f)") }
    exit(1)
}

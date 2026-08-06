# Native clients

**Status: a client library each, and no app.** The Swift package compiles and
its assertions run — `cd ios && swift run omt-client-check` — and the Kotlin
equivalent is written with its own tests. What does not exist is either
*application*: no UI, no socket lifecycle, no push registration, nothing built
against a device. The web client is the one that works, and it is what you
should use today.

## Why these exist at all

The web client is a real client: it reflows, it takes touch, it receives push,
and on Android it installs. Two things it cannot do, and they are the entire
reason for a native shell:

- **iOS push requires the user to install the PWA to the home screen first.**
  Nobody does this. A missed "needs your answer" is the failure the whole
  product exists to prevent, so on iOS the notification is worth an app.
- **Background reconnection.** A browser tab is suspended aggressively. A
  native client can hold a connection, or at least reconnect on a push and
  show the card before the user has finished unlocking the phone.

Nothing else justifies one. In particular, **running agents locally on a
phone does not** — that was researched and rejected in
[`docs/research/mobile-native.md`](../docs/research/mobile-native.md), and
nothing in this directory reopens it.

## The shape

Both apps are **thin clients over the same wire protocol as the browser**.
There is no second protocol, no mobile-specific endpoint, and no capability
that exists for a phone and not for a terminal — that would break the parity
gate, which is the point of the parity gate.

```
omt daemon ──ws──┬── web/src        the client that works
                 ├── mobile/ios     a shell around the same protocol
                 └── mobile/android same
```

What each native app is allowed to own: the socket lifecycle, the push
registration, the keychain the token lives in, and the widgets. What it must
not own: any decision about *what a card means*. Answerability comes from
`Deliverable`, ordering comes from the same rules as `threads.ts`, and a
native client that reimplements either will disagree with the web client about
whether a button should exist — which is exactly the class of bug the
tier ladder was built to make impossible.

## The rule that decides the port

Anything decidable is written once and tested once. Where that lives is the
open question, and it is deliberately still open:

| Option | What it costs |
|---|---|
| Reimplement per platform | Two more places for the answerability rule to drift. Rejected. |
| Share the TypeScript over a JS runtime | A runtime per app, and the ordering rules run where they were tested. |
| Compile the Rust to a library | `omt-proto` and `omt-agent` already hold these rules and are already tested. Largest build change; smallest amount of new logic. |

The third is the one to beat, because the rules already exist in Rust and are
already under test — but it is not chosen here, and choosing it is the first
task of whoever builds these apps rather than a decision to inherit silently
from a scaffold.

## Layout

- `shared/` — the contract both apps target, kept as generated artifacts so
  there is one source for what a capability is
- `ios/` — SwiftUI shell, notification service extension
- `android/` — Compose shell, foreground service for the socket

## Before either of these is worth writing

The web client has to be finished first, and one thing is not: the terminal
screen renders text rather than a grid (see the mobile design's status table).
A native app around a client that cannot show a terminal properly is a native
app around the wrong thing.

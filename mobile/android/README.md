# The Android client

**Written, not built.** The protocol, the roster rules and the Compose UI are
here with their own tests. What has not happened is a compile: this needs the
Android SDK and a Gradle toolchain, and the machine it was written on has
neither.

That is a real difference from the iOS side, which does compile —
`cd ../ios && swift run omt-client-check` runs sixteen checks — and it is
stated rather than glossed over, because "the code is written" and "the code
works" are not the same claim.

## To build it

```sh
gradle wrapper          # or use Android Studio, which does this for you
./gradlew test          # the roster and protocol rules
./gradlew assembleDebug # the app
```

## What is here

- `Protocol.kt` — the wire: which capabilities are commands, and how the token
  travels. A command carries an intent id and a query must not; the daemon
  refuses a command without one.
- `Roster.kt` — the ordering, which is the whole mobile argument: what needs a
  human is first, whatever it is called and whenever it started.
- `MainActivity.kt` — the Compose roster.

The capability list in `Protocol.kt` is hand-written while the browser's is
generated. That is a difference to remove rather than live with: it belongs in
`cargo xtask codegen` beside the TypeScript one, committed and diffed in CI.
A hand-written list is how a client comes to believe in a capability the server
does not offer, and the symptom is a button that does nothing.

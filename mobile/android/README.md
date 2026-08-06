# The Android client

**It builds.** `gradle test` runs six tests over the protocol and roster rules,
and `gradle assembleDebug` produces an APK.

```sh
gradle test             # the roster and protocol rules
gradle assembleDebug    # app/build/outputs/apk/debug/app-debug.apk
```

`local.properties` points at your Android SDK and is not committed, because it
is a path on one machine. `gradle.properties` pins the JDK on purpose: Kotlin's
compiler does not support every JDK a machine might default to, and the failure
it produces is `Daemon compilation failed: null`, which says nothing about the
cause. That cost half an hour once; the comment is there so it costs nobody
else any.

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

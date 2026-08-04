# What both native clients target

Nothing is hand-written here. The contract is generated from the same
capability catalog the server registers, which is what makes a client that
compiles a client that agrees with the instance:

- `web/src/generated/catalog.ts` — every capability, its role and its kind
- `docs/reference/capabilities.md` — the same table, for humans

A native client that needs a Swift or Kotlin version of that catalog should
generate it in `cargo xtask codegen` alongside the TypeScript one, and commit
the output, and diff it in CI — the same way every other generated artifact in
this repository is handled. Writing it by hand produces a client that believes
in a capability the server does not offer, and the symptom is a button that
does nothing.

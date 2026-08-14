# Adapters for every agent the architecture names

## Why

`docs/architecture/06-agent-layer.md` names the agents omt supports and the
tier each one reaches. The code covered eight of the ten: Claude Code by hook,
four over generic ACP, three on the heuristic floor. **Codex and Cursor had no
adapter at all** — omt would run either of them, detect nothing, and show a
pane with no state, no transcript and no answerable card, with nothing to say
why.

That gap was invisible because the registry test asserted a subset of the
matrix rather than the matrix. A doc row without an adapter shipped as a
feature that quietly is not there.

## What changes

- A dedicated `Codex` adapter at hook tier, wiring `CODEX_NOTIFY` at spawn.
- A dedicated `Cursor` adapter at hook tier, mapping its lifecycle hooks.
- The registry test now requires every agent in the matrix, so the next doc row
  without an adapter fails a test rather than shipping.

## What this deliberately does not do

Codex's `app-server` is a JSON-RPC dialect of its own rather than ACP, and omt
has no client for it. This change does **not** claim protocol tier for Codex.
A tier is a promise about what a surface will receive, and claiming one that
nothing populates turns on views that stay empty — the exact failure the tier
ladder exists to prevent. The app-server client is separate work.

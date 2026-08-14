# Design

## Why dedicated adapters rather than more generic-ACP rows

`GenericAcp` covers four agents because they speak one protocol; the adapter is
genuinely the same code. Codex and Cursor do not speak it. Their knowledge is
what makes them different from each other: which environment variable states
their identity, what their hook entry point is called, how their events are
spelled. That is exactly what `AgentAdapter` is for, and folding it into a
generic case would mean a `match` on agent kind inside the generic adapter —
the thing the registry exists to avoid.

## Fingerprints: env markers over executable names

Both adapters lead with environment markers and fall back to executable names.
Four of these agents ship as Node bundles, and an executable called `node`
running a bundle called `cli.js` describes several of them at once. A variable
the agent sets itself is the agent saying who it is; a name is a resemblance.

## Cursor: the prompt hook has to start the turn

Cursor has no separate turn-start hook. If `beforeSubmitPrompt` only recorded
the message, a session would read as idle for the entire time it was working —
which is worse than no state at all, because a phone would show green while an
agent burned tokens. So that hook emits both the message and the turn start.

## Codex: hook tier, stated

Codex's `app-server` would be a better source and omt has no client for it. The
tier-ladder test refuses an adapter that declares native mode without a spawn,
and it caught this when the first draft claimed protocol tier — which is the
test working. The adapter reports hook tier until the client exists.

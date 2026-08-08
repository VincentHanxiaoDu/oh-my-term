# What a session is spending

## Why

Running out of quota mid-task, with no warning, is a specific and common
annoyance. Several agent CLIs report their own usage and rate limits over the
channels omt already reads — and omt currently drops every one of them.

The machinery is not missing. `omt-agent/src/usage.rs` has `Usage`, `RateLimit`,
`Headroom` and a `UsageLedger` that accumulates per session, all tested. The
Codex adapter already decodes a rate-limit notification. **Nothing holds the
ledger and no capability exposes it**, so every token counted is counted into a
value that is dropped a line later.

This is the cheapest item on the roadmap by a wide margin: the events arrive,
the accumulator exists, and what is missing is the wiring and one query.

## What changes

- The instance holds a `UsageLedger` and feeds it every agent payload it
  already routes.
- `usage.report` — what a session has spent, what every session has spent
  together, and the headroom the agent reported.
- Adapters emit `Usage` where their protocol carries it. Codex already decodes
  its rate limits into an activity guess and throws the numbers away; that
  becomes a real `RateLimit`.

## What this deliberately does not do

**No estimated cost.** `Usage::cost_usd` is populated only when the agent
itself states one. Multiplying tokens by a price table omt maintains would
produce a number that is wrong the week any provider changes pricing, shown
next to numbers that are right — and a user cannot tell which is which.

**No warning threshold, no budget, no cutoff.** Reporting headroom is useful;
deciding what to do about it is a policy, and a policy belongs in a
configuration key or a plugin rather than being invented here. `Headroom` keeps
`Unknown` distinct from a low number precisely so a surface can decline to draw
a bar rather than drawing an empty one.

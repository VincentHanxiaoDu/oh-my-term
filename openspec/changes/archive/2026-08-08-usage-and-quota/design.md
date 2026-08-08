# Design

## The ledger lives with the instance

`UsageLedger` is per instance, keyed by session, because that is the shape of
the question people ask: what is *this* session spending, and what am I spending
in total. It is fed from the one place agent payloads are already routed, so
there is no second path for a payload to arrive by and no chance of counting one
twice.

It is bounded by the session tree rather than by a limit of its own: a session
that is gone takes its entry with it.

## Cost is reported, never computed

`cost_usd` is `Option`, and omt fills it only from what an agent said. The
alternative is a price table in omt that goes stale the week any provider
changes pricing — and the failure is worse than being absent, because the
estimate would sit next to counts that *are* right and nothing distinguishes
them.

## `Unknown` is a value, not a missing one

`Headroom::Unknown` is the common case: most agents say nothing about limits
most of the time. Collapsing it into "100% remaining" would draw a full bar for
an agent that has never mentioned a limit, and collapsing it into zero would
draw an empty one. Both are inventions. A surface that receives `Unknown` should
draw nothing, which it can only do if the value survives the wire.

## Adapters stop flattening what they already decode

The Codex adapter turns `account/rateLimits/updated` into a generic "busy"
activity guess — it decodes the notification and discards the contents. That
becomes a `RateLimit` carrying the agent's own words. The tier ladder permits
it: this is a protocol source, told rather than inferred.

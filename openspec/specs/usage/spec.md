# usage Specification

## Purpose
TBD - created by archiving change usage-and-quota. Update Purpose after archive.
## Requirements
### Requirement: Usage accumulates per session and in total

The instance MUST accumulate the usage an agent reports, per session and across
all of them. A count that is observed and dropped is the same as not observing
it.

#### Scenario: An agent reports token usage
- **WHEN** an agent payload carrying usage is routed for a session
- **THEN** that session's totals increase by the reported amounts
- **AND** the instance-wide totals increase by the same

#### Scenario: A session that has reported nothing
- **WHEN** usage is asked for a session that reported none
- **THEN** it reports zero rather than failing

### Requirement: Headroom distinguishes silence from a low number

Reported headroom MUST be distinguishable from no report at all. A surface that
rendered silence as an empty bar would invent the one number a user acts on.

#### Scenario: The agent said nothing about limits
- **WHEN** headroom is asked for and no rate limit has been reported
- **THEN** it reports that the limit is unknown
- **AND** does not report a percentage

#### Scenario: The agent reported a limit
- **WHEN** a rate limit has been reported for a session
- **THEN** headroom carries what the agent said, including when it resets

### Requirement: Cost is only ever what the agent stated

omt MUST NOT compute a cost from token counts. A cost appears only when the
agent itself reported one.

#### Scenario: An agent reports tokens but no cost
- **WHEN** usage arrives with token counts and no cost
- **THEN** the reported cost is absent rather than estimated

### Requirement: An agent's own rate-limit report reaches the ledger

An adapter that receives a rate limit over its protocol MUST report it as a rate
limit rather than flattening it into an activity guess.

#### Scenario: A protocol carries a rate-limit update
- **WHEN** the adapter normalises a rate-limit notification
- **THEN** it emits a rate limit carrying what the agent said


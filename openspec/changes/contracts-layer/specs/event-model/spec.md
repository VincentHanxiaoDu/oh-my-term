## Purpose

The single stream through which every surface learns what changed — one
envelope, one closed vocabulary of kinds and sources, and the agent and
interaction payloads that the TUI, the API and the web client all render from
the same data.

## ADDED Requirements

### Requirement: One event stream, no internal channel

State changes SHALL be broadcast once, in one envelope, to every subscriber. The
native TUI subscribes on the same terms as a remote client.

#### Scenario: The TUI sees exactly what the API can see

- **WHEN** any state change occurs
- **THEN** the resulting event is available to the TUI and to remote clients with
  the same schema and the same identifiers
- **AND** no event exists that only the TUI can observe

### Requirement: Every event is ordered within its scope

Each event SHALL carry a monotonically increasing sequence number within its
scope, so that a reconnecting client can resume exactly and can detect a gap.

#### Scenario: Session, workspace and instance scopes each have their own space

- **WHEN** events are emitted for a session, for a workspace, and for the
  instance as a whole
- **THEN** each is numbered within its own scope
- **AND** a client resuming one scope is unaffected by traffic in another

#### Scenario: A client resumes without loss or duplication

- **WHEN** a client reconnects and supplies the last sequence number it
  processed
- **THEN** it receives exactly the events after that point, in order

#### Scenario: A gap is announced, never silent

- **WHEN** the requested resume point is older than what the instance retains
- **THEN** the client is told it must resynchronize, and how much was dropped
- **AND** the client is never left believing it is current when it is not

#### Scenario: A sequence position is never reused

- **WHEN** a position has been issued for a scope
- **THEN** no later event in that scope is issued the same or a lower position,
  for the lifetime of that scope's identity
- **AND** a client presenting a position from the future is recognizable as such
  rather than silently accepted

### Requirement: Kind and source vocabularies are closed

Every event SHALL declare its kind from a closed set and its source from a
closed set, so that a client can filter a subscription without receiving traffic
it cannot name.

#### Scenario: An event with no kind cannot exist

- **WHEN** an event is emitted
- **THEN** it carries a kind drawn from the closed set
- **AND** it is therefore reachable by a subscription filter

#### Scenario: Source identifies the confidence tier that produced it

- **WHEN** an agent event is emitted
- **THEN** its source names the observation tier that produced it
- **AND** the tier recorded on the event matches the tier of the reporting source

### Requirement: Agent activity is a normalized stream

Everything omt observes about an agent — lifecycle, turns, content, tool calls,
usage, queue changes and interactions — SHALL be expressed in one normalized
event vocabulary that is independent of which agent produced it and of which
observation tier saw it.

#### Scenario: A surface renders a conversation without agent-specific code

- **WHEN** a client renders an agent session's history
- **THEN** it consumes the normalized stream
- **AND** it needs no knowledge of the underlying agent's own formats

#### Scenario: Heuristic sources cannot fabricate structure

- **WHEN** the only available source for a session is screen heuristics
- **THEN** it may report a coarse activity guess
- **AND** it cannot produce structured content such as an interaction

### Requirement: An interaction is an addressable, resolvable object

A request from an agent for a human decision SHALL be represented as an object
with a stable identity, a kind, a lifecycle state, and an explicit statement of
whether and how omt can deliver an answer to it.

#### Scenario: Answerability is explicit, not inferred from openness

- **WHEN** a surface renders an interaction
- **THEN** whether it offers an answering affordance is determined by the
  interaction's declared deliverability
- **AND** an interaction omt cannot safely answer is never presented as
  answerable

#### Scenario: An undeliverable interaction explains itself

- **WHEN** an interaction cannot be answered remotely
- **THEN** it carries the reason, and the surface shows that reason together
  with a route to the terminal

#### Scenario: Writing an answer is distinct from the answer taking effect

- **WHEN** an answer has been written toward the agent but not yet observed to
  have been recorded
- **THEN** the interaction is in a state distinct from both unanswered and
  successfully answered
- **AND** no surface presents that state as success

#### Scenario: A committed decision records what was decided

- **WHEN** an answer has been accepted but not yet written toward the agent
- **THEN** the recorded state carries the response itself
- **AND** an interruption at that moment can report what the answer was, rather
  than only that one existed

#### Scenario: A lost answer preserves what was lost

- **WHEN** an answer is written and no confirming observation arrives within the
  bounded window
- **THEN** the interaction reports non-delivery, retains the response that was
  attempted, and is not retried automatically

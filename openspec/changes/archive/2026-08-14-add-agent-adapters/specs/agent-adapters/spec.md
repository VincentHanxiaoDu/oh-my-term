## ADDED Requirements

### Requirement: Every agent in the architecture matrix has an adapter

The built-in adapter set MUST contain an adapter for every agent named in the
architecture's support matrix. An agent named in the docs with no adapter is a
pane that reports nothing, with nothing to explain why.

#### Scenario: The matrix is covered
- **WHEN** the built-in adapter set is built
- **THEN** it resolves an adapter for Claude Code, Codex, Cursor, opencode,
  Gemini CLI, Qwen, Goose, Aider, Amp and Crush

#### Scenario: A new documented agent without an adapter fails the build
- **WHEN** an agent is added to the matrix and no adapter is registered for it
- **THEN** the registry test fails rather than the gap shipping

### Requirement: An adapter states only the tier it can deliver

An adapter MUST report the highest tier omt can actually reach for that agent
with the code that exists, not the highest tier the agent is capable of. A tier
is a promise about what a surface will receive.

#### Scenario: A protocol exists but no client for it does
- **WHEN** an agent exposes a native protocol omt has no client for
- **THEN** the adapter reports the tier its implemented channel reaches
- **AND** it does not declare native session mode

#### Scenario: Declaring native mode requires a way to start it
- **WHEN** an adapter declares native session mode
- **THEN** it also supplies the spawn describing how to start that channel

### Requirement: Spawn injects the session correlation

Every adapter MUST inject the variables that let a hook know which pane it
belongs to, which is what removes the class of heuristics that match a
transcript to a pane after the fact.

#### Scenario: An agent omt started can be correlated
- **WHEN** omt spawns any agent through its adapter
- **THEN** the environment carries the instance, session and socket
- **AND** where the agent has a hook entry point, it is pointed at omt's

### Requirement: An unrecognised event is reported, never dropped

An adapter MUST return an error naming the agent and the verbatim event when it
has no mapping for it. A silent drop is a gap nobody can find.

#### Scenario: An agent ships a new event
- **WHEN** an event arrives that the adapter has no mapping for
- **THEN** the adapter returns an error carrying the agent and the event name

## ADDED Requirements

### Requirement: An agent can report over the terminal it was started in

omt SHALL read structured agent events published as `OSC 777 ; notify` with
omt's marker, normalize them through the same per-agent mapping a hook uses, and
record them at `Marker` tier.

The only structured source that needs nothing installed: hooks require writing
to a file the agent owns, a transcript tailer requires knowing where that file
is, and this requires only that the agent was started.

#### Scenario: A report from an identified agent

- **WHEN** an agent in a pane publishes an event omt's adapter for it
  recognizes
- **THEN** it is recorded against that session
- **AND** it is recorded at `Marker` tier, never at `Hook`

Bytes on a pty are not the same evidence as a socket whose peer the kernel
vouched for, and anything running in that pane can write them.

#### Scenario: A report from a pane with no identified agent

- **WHEN** the report names its own agent and omt has not identified one there
- **THEN** nothing is recorded

Believing the name in the message is how one pane's bytes put words into
another agent's conversation.

#### Scenario: A payload that is not usable

- **WHEN** the payload is not JSON, or carries no event name, or names an event
  the adapter has no mapping for
- **THEN** nothing is recorded and nothing is said

#### Scenario: JSON contains the separator

- **WHEN** the payload contains the character the sequence's fields are split on
- **THEN** the payload is rejoined and read whole

#### Scenario: Saying the channel exists

- **WHEN** omt spawns a pty
- **THEN** the environment carries the version of this channel that omt reads

There is no reply on this channel to negotiate with, and an agent left guessing
either stays silent or writes JSON onto somebody's screen.

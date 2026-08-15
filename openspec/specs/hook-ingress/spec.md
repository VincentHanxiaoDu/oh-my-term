# hook-ingress Specification

## Purpose
TBD - created by archiving change close-the-unwired-surfaces. Update Purpose after archive.
## Requirements
### Requirement: An agent's hook reports reach the instance that owns its session

omt SHALL accept `HookEvent` on its local socket, normalize it through the
adapter for that agent, and record the result against the session it names.

The flagship observation path. Everything downstream of it — transcripts, cards,
thread rosters, the tier ladder's `Hook` rung — existed and had nothing feeding
it, so an agent running inside omt reported nothing at all.

#### Scenario: A question reaches every surface

- **WHEN** an agent's hook reports a structured question for a known session
- **THEN** it is recorded in that session's transcript
- **AND** a card is raised on the ledger
- **AND** the session's agent reports that somebody is being waited for

#### Scenario: The agent is never held up

- **WHEN** anything goes wrong absorbing a report — an unknown event, a session
  this instance does not have, a poisoned lock
- **THEN** the hook is still told to proceed, with the nonce it asked with

#### Scenario: An unattributed report

- **WHEN** a hook cannot name its session
- **THEN** nothing is recorded anywhere

A wrongly-attributed observation puts one agent's question in another agent's
conversation, and nothing downstream can detect it afterwards.

#### Scenario: Another instance's session

- **WHEN** a hook names a session this instance has never seen
- **THEN** nothing is recorded, and the hook is told to proceed

### Requirement: A process that owns sessions listens for their hooks

omt SHALL bind a socket whenever it owns running sessions, including when it is
drawing a terminal, and SHALL put that socket's path in the environment of
every pty it spawns.

A hook with no `$OMT_SOCK` exits at its guard clause without connecting, which
is indistinguishable from a hook that was never installed.

#### Scenario: A terminal serving beside its own frame loop

- **WHEN** somebody runs omt as a terminal and starts an agent in a pane
- **THEN** that agent's hooks reach this instance
- **AND** this process does not run a second pump, restore or autosave

#### Scenario: A hook fired from inside a pane

- **WHEN** the hook binary runs in a pane omt spawned, with no arguments beyond
  its agent
- **THEN** it finds the socket and the session from its environment alone


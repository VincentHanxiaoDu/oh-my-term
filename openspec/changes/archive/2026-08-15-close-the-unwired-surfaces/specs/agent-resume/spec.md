## ADDED Requirements

### Requirement: An agent can be continued rather than restarted

omt SHALL report the command that would continue an agent's own conversation,
using the id that agent gave it.

An agent restarted with the same command is a fresh agent with none of the
context, which from the outside is indistinguishable from the session having
survived.

#### Scenario: An agent omt knows how to resume

- **WHEN** a session's agent has reported its own session id
- **THEN** the plan is that agent's own spelling of "continue this conversation"

#### Scenario: An agent omt cannot resume

- **WHEN** omt has no resume command for that agent
- **THEN** the plan is empty and says so

A guessed flag starts a new conversation while claiming to continue one, and
the person finds out from the agent's answer.

#### Scenario: An agent that has not named its conversation

- **WHEN** no hook has reported the agent's own session id
- **THEN** the plan is empty and says that, which is a different situation from
  the one above

#### Scenario: Saying, not doing

- **WHEN** a resume plan is asked for
- **THEN** nothing is started

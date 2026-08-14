## ADDED Requirements

### Requirement: An agent that announces itself is heard

omt SHALL read the OSC title and OSC progress a pane's program sets, and use
them to identify the agent and its state.

This is the agent deliberately telling its host what it is, which is the
strongest evidence available without cooperation, and it needs no permission,
survives ssh, and does not depend on how the binary was launched or signed.

#### Scenario: An agent sets the terminal title

- **WHEN** a pane's program sets the title to a string that names a known agent
- **THEN** omt reports that agent for that pane
- **AND** the reading is above the heuristic floor and below a hook or protocol

#### Scenario: The state glyph in the title

- **WHEN** the title carries a state glyph the agent uses for working or idle
- **THEN** omt reports that state
- **AND** it never reports the glyph itself to a person

#### Scenario: A title that names nothing

- **WHEN** the title is a directory, a command, or anything omt does not
  recognise
- **THEN** omt reports no agent from the title, rather than guessing from it

### Requirement: A probe that cannot read everything still reports what it read

The process probe SHALL report a process's name even when its arguments and
environment cannot be read.

A signed, hardened binary refuses `KERN_PROCARGS2` to a process the same user
owns. The shipped Claude Code binary is one, so the probe returned nothing at
all for the commonest agent there is.

#### Scenario: A hardened binary in the foreground

- **WHEN** the foreground process's argv and environment cannot be read
- **AND** its name matches a known agent
- **THEN** omt reports that agent

#### Scenario: Nothing readable at all

- **WHEN** neither the name nor the arguments can be read
- **THEN** omt reports no agent, rather than a default

### Requirement: What is on the screen is read by region, not by search

Screen-derived state SHALL be matched against named regions of the pane's grid
with per-agent rules, priorities, and negation.

Grepping a whole screen for a word finds it in scrollback, in a diff, and in a
file the agent is showing. A region is what makes the difference between "this
agent is asking a question now" and "the word yes appears somewhere".

#### Scenario: A prompt that is waiting for a person

- **WHEN** the region an agent puts its questions in matches a blocking rule
- **THEN** omt reports the agent as waiting on a human

#### Scenario: The same words in the scrollback

- **WHEN** the same text appears outside that region
- **THEN** omt does not report the agent as waiting

#### Scenario: A rule that would fire on a transcript view

- **WHEN** the agent is showing its own history rather than its live prompt
- **THEN** omt leaves the state as it was, rather than reading the history as
  the present

### Requirement: Finished and unseen is its own state

omt SHALL distinguish an agent that finished while nobody was looking from one
that is idle and has been seen.

This is the state the product exists for. Collapsing it into "idle" is how
somebody misses that the thing they were waiting for is done.

#### Scenario: An agent finishes while its pane is not on screen

- **WHEN** an agent goes idle and its pane has not been looked at since
- **THEN** omt reports it as done rather than idle

#### Scenario: Looking at it

- **WHEN** somebody focuses the pane
- **THEN** it becomes idle, and stays idle

### Requirement: omt never tells a person "unknown"

No surface SHALL show the word "unknown" for an agent's state.

"Unknown" describes omt's knowledge, not the agent, and beside an agent omt has
just found it reads as "omt has no idea what this is" — which is what it was
read as.

#### Scenario: An agent whose state has not been established

- **WHEN** omt knows an agent is present but not what it is doing
- **THEN** the surface says what it does know, and does not print "unknown"

### Requirement: A test cannot pass because of the environment it runs in

Every harness that exercises agent detection SHALL strip inherited agent
environment before starting omt.

Three tests passed — one of them driving the real agent binary — because the
suite runs inside an agent session and `CLAUDECODE` was inherited by every
process the harness spawned. The probe matched the harness's own environment.

#### Scenario: The suite runs inside an agent session

- **WHEN** the harness starts omt
- **THEN** no agent environment variable from the parent reaches it

#### Scenario: A detection test with no agent running

- **WHEN** a plain shell runs in the pane
- **THEN** omt reports no agent

# agent-observation Specification

## Purpose
TBD - created by archiving change observe-agents-the-way-terminals-do. Update Purpose after archive.
## Requirements
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

### Requirement: The agent's session file is read, and its location is never guessed

omt SHALL read an agent's own session file when the agent has reported where it
is, and SHALL NOT derive that location from the working directory or from
choosing the newest file in a directory.

A derived path is a guess, and it is wrong precisely when a machine is running
more than one agent in one directory — which is the ordinary case for a session
tree, not an edge case. Reading the wrong file puts another conversation's words
into this session.

#### Scenario: The agent reports where its file is

- **WHEN** a hook reports a transcript path for a known session
- **THEN** omt reads that file from that point on

#### Scenario: No path was ever reported

- **WHEN** no source has reported a transcript path for a session
- **THEN** omt reads no file for it
- **AND** does not derive a candidate path from the working directory

#### Scenario: The path names a file that is not there

- **WHEN** the reported path cannot be opened
- **THEN** omt observes nothing from it and continues to serve the session

### Requirement: The session file is read from a position, and a torn line is not lost

The reader SHALL keep a byte position per file, resume from it, hold a trailing
line that has no newline yet, and restart from the beginning when the file is
shorter than the position.

A transcript reaches hundreds of megabytes; re-reading it on every change makes
a session slower the longer somebody works. A record appended in two writes is
routinely caught mid-line — parsing it drops a record, and skipping it drops the
record permanently, because the position has already moved past it. A file
shorter than the position is a different file, not a rewound one.

#### Scenario: Only what is new is read

- **WHEN** the file has grown since the last read
- **THEN** only the appended bytes are read

#### Scenario: A record arrives in two writes

- **WHEN** the file ends without a newline
- **THEN** the incomplete line is not parsed
- **AND** it is read as one record once its newline arrives

#### Scenario: The file was replaced

- **WHEN** the file is shorter than the position held for it
- **THEN** the reader starts again from the beginning of the file

#### Scenario: A line that is not a record

- **WHEN** a line cannot be parsed
- **THEN** it is skipped and the rest of the file is still read

### Requirement: The session file's records reach every surface at their own tier

Payloads read from a session file SHALL be reported at `Tier::Transcript` with
the `Transcript` source tag, through the same absorption as the hook path, so
the conversation, the ledger, the thread roster and the event stream all see
them.

The tier is below a hook and above a marker: it is the agent's own writing,
which is why it outranks anything inferred, and it is written after the fact,
which is why the hook's account of the same event wins.

#### Scenario: A conversation that predates omt

- **WHEN** omt begins reading a session file that already holds a conversation
- **THEN** that conversation is available on omt's surfaces

#### Scenario: Attribution is never taken from the file

- **WHEN** a record is read for a session
- **THEN** the agent it is attributed to is the one already observed for that
  session

### Requirement: What a stronger source already reported is not reported twice

omt SHALL suppress a payload read from a session file when the same event has
already been observed for that session from any source.

The hook and the file describe the same tool call. A conversation showing both
shows every call twice, and a count of what an agent did would double.

#### Scenario: A tool call already known from a hook

- **WHEN** a session file record carries a tool call whose id a hook already
  reported
- **THEN** the record adds nothing to the conversation

#### Scenario: The same file is read twice

- **WHEN** a record already read is read again
- **THEN** it is not appended a second time


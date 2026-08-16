# agent-observation Specification

## ADDED Requirements

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

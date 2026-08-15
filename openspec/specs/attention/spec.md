# attention Specification

## Purpose
TBD - created by archiving change close-the-unwired-surfaces. Update Purpose after archive.
## Requirements
### Requirement: An agent that needs a person reaches one who is looking elsewhere

omt SHALL raise a notification through the terminal it is drawing on, and SHALL
be able to play a sound, when an agent's state changes to one worth interrupting
somebody for.

Every other surface omt has answers "who needs me" to somebody already looking
at the screen, which is the one case where the answer did not need finding.

#### Scenario: Being waited for

- **WHEN** an agent's state becomes blocked
- **THEN** a notification is raised, whether or not that pane is focused

A question somebody can see is still a question that has stopped the work.

#### Scenario: The same state reported again

- **WHEN** a source re-reports a state that has not changed
- **THEN** nothing is raised

#### Scenario: Finishing while somebody watches

- **WHEN** an agent goes idle in the pane that is focused
- **THEN** nothing is raised

#### Scenario: A terminal that cannot notify

- **WHEN** the terminal is not one omt recognizes
- **THEN** nothing at all is written

A sequence an emulator does not understand is drawn as text, in the middle of
somebody's work.

#### Scenario: The agent's own words

- **WHEN** the text of a notification contains an escape, a bell or a string
  terminator
- **THEN** they are removed before it is sent

#### Scenario: Inside a multiplexer

- **WHEN** omt is running under a multiplexer
- **THEN** the sequence is wrapped so it reaches the outer terminal

### Requirement: omt ships no audio

omt SHALL play a sound the machine already has, or a file somebody named, and
SHALL be silent when there is neither.

#### Scenario: A machine with no audio

- **WHEN** nothing on the machine can play a file
- **THEN** the notification is still raised and nothing is said about the sound

#### Scenario: A sound somebody chose

- **WHEN** a file is named in the configuration
- **THEN** it is played instead of the system's own

### Requirement: A program asking what colour the terminal is gets an answer

omt SHALL answer `OSC 10 ; ?` and `OSC 11 ; ?` with the colours it has been
told, and SHALL say nothing when it has not been told any.

Agent CLIs ask this to decide whether they are on a light or a dark terminal.
Unanswered they guess, and what somebody sees is the *agent* rendering
unreadably inside omt and correctly everywhere else.

#### Scenario: omt knows

- **WHEN** a program in a session asks for the background colour
- **AND** omt has been told what it is
- **THEN** it is answered

#### Scenario: omt does not know

- **WHEN** omt has not been told
- **THEN** nothing is written

#### Scenario: Setting rather than asking

- **WHEN** a program sets the background colour instead of asking
- **THEN** no reply is sent

### Requirement: A program inside a pane can reach somebody who left the window

omt SHALL turn `OSC 9` and `OSC 777 ; notify` from a session into an event, and
SHALL pass it to the terminal it is drawing on.

The long build that finishes while its window is behind a browser. omt is the
terminal that program is talking to; a multiplexer that swallows these is why
people run their builds outside one.

#### Scenario: A program asks for attention

- **WHEN** a program in a pane emits a notification sequence
- **THEN** a notification is raised on the terminal omt is drawing on

#### Scenario: A title and a body

- **WHEN** the sequence carries both
- **THEN** they are kept apart rather than concatenated

#### Scenario: An empty notification

- **WHEN** the sequence carries nothing
- **THEN** nothing is raised


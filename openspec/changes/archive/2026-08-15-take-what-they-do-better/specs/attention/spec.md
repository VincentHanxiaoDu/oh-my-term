## ADDED Requirements

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

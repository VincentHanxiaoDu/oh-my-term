# terminal-streaming Specification

## Purpose
TBD - created by archiving change sessions-outlive-the-terminal. Update Purpose after archive.
## Requirements
### Requirement: A client can subscribe to a session's output

The instance SHALL push a subscribed session's output to that client as binary
frames, without the client asking again.

Polling for a screen is how a terminal ends up a fifth of a second behind the
program it is drawing, and asking for the whole screen to find out that one
character changed is the cost that makes it worse over a link.

#### Scenario: Live output

- **WHEN** a program in a subscribed session writes
- **THEN** those bytes reach the subscriber

#### Scenario: Replies still work

- **WHEN** a subscriber makes a capability call while output is streaming
- **THEN** it receives the reply, whole, and no frame is interleaved inside
  another

### Requirement: A client that has just arrived is seeded, not replayed

The instance SHALL send a newly subscribed client a rendering of the session's
current state, and SHALL seed again rather than send a gap when a client has
fallen behind what it retains.

Re-sending the tail of a raw byte stream is how a retry becomes a disaster: the
bytes are not a document, they are commands, and half of a command is not half
of an effect.

#### Scenario: Attaching to a session that has been running

- **WHEN** a client subscribes to a session that has already produced output
- **THEN** it receives a seed that reproduces what is on that session's screen

#### Scenario: Falling too far behind

- **WHEN** a client's position is older than what the instance still holds
- **THEN** it is seeded again rather than sent the bytes that remain

### Requirement: The seed is bytes like everything else

The rendering SHALL be escape sequences a terminal emulator can consume, so that
seeding and live output are one path.

Two paths to reconstruct a screen means one of them is the one nobody tests.

#### Scenario: What a seed reproduces

- **WHEN** a seed is fed to a fresh emulator of the same size
- **THEN** its visible text matches the session's
- **AND** so do the colours and the cursor position


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

### Requirement: The configured scrollback is the scrollback

A session SHALL keep the number of scrollback lines the configuration asks for.

It kept the built-in ten thousand whatever the setting said, while the settings
screen reported the user's number and the file it came from — which is a
setting that is accepted, listed, traced, and ignored.

#### Scenario: A configured cap

- **WHEN** `terminal.scrollback_lines` is set and a session is started
- **THEN** that session keeps that many lines

#### Scenario: No setting

- **WHEN** nothing is set
- **THEN** the built-in default applies, and omt starts normally

### Requirement: The configured shell is preferred

A new pane SHALL start `terminal.default_shell` when it is set, and `$SHELL`
otherwise.

#### Scenario: A configured shell

- **WHEN** `terminal.default_shell` names a shell
- **THEN** a new pane runs it

#### Scenario: An empty setting

- **WHEN** the setting is empty or blank
- **THEN** it is ignored rather than run as a program with no name

### Requirement: A bell in a pane is the configured bell

The Bell event SHALL be consumed according to `terminal.bell`.

It was emitted by the daemon and consumed by no one, so a bell did nothing
whatever the setting said.

#### Scenario: Audible

- **WHEN** the setting is `audible` or `both` and a pane rings
- **THEN** the outer terminal's bell is written

#### Scenario: Off

- **WHEN** the setting is `off`
- **THEN** nothing is written and nothing is flashed

### Requirement: A large paste is asked about

A paste that exceeds `paste_confirm_lines`, or that ends in a newline when
`paste_confirm_on_newline` is on, SHALL wait for an answer rather than reach
the program, and y SHALL hand it over with any other key dropping it.

It reached the program immediately, so a two-hundred-line pasted script ran
its first line before anything could look at it.

#### Scenario: Over the threshold

- **WHEN** a paste has more lines than the threshold
- **THEN** it is held, the hint row names it and asks, and y sends it

#### Scenario: Answered anything else

- **WHEN** the answer is not y
- **THEN** the paste is dropped and the program sees nothing

### Requirement: Blocks open only where they may

When `terminal.blocks.heuristic_fallback` is off, unmarked output SHALL NOT
open a block, and marked commands SHALL.

#### Scenario: Fallback off, marks on

- **WHEN** the fallback is off and a marked command runs
- **THEN** exactly one attributed block exists

### Requirement: Reflow is what the setting says

`reflow.on_resize` SHALL decide what a resize re-writes. `none` cuts the screen
at the new size; the other two reflow, which in this model is the same
operation — the scrollback is width-independent by construction.

#### Scenario: None

- **WHEN** the setting is `none` and a resize changes the width
- **THEN** the screen is cut and history scrolled away is gone

### Requirement: A new pane starts where the setting says

`terminal.default_cwd` SHALL choose the directory a new pane starts in.

#### Scenario: Home

- **WHEN** the setting is `home`
- **THEN** the pane starts in `$HOME`

#### Scenario: Workspace, the default

- **WHEN** the setting is `workspace`
- **THEN** the pane starts in the directory omt was started in


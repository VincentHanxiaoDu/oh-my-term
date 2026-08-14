# mobile Specification

## Purpose
TBD - created by archiving change mobile-first-client. Update Purpose after archive.
## Requirements
### Requirement: Typing works on a device with no physical keyboard

The terminal screen MUST contain a focusable element that receives text, so the
device's own keyboard opens. A key listener on the document is not sufficient:
a phone opens no keyboard for it.

#### Scenario: The terminal is opened on a touch device
- **WHEN** the terminal screen is shown
- **THEN** a focusable input exists and holds focus
- **AND** the device keyboard can open

#### Scenario: A character is typed
- **WHEN** text is entered into that input
- **THEN** it is sent to the session and the input is cleared
- **AND** the input never accumulates what was already sent

#### Scenario: The user taps the terminal
- **WHEN** the terminal body is tapped
- **THEN** the input takes focus, so the keyboard returns after it was dismissed

### Requirement: The terminal's state has one owner

Only the terminal screen decides what the terminal shows. An update about
something else MUST NOT replace drawn content with a loading state.

#### Scenario: An unrelated store update arrives
- **WHEN** the session list refreshes while a terminal is drawn
- **THEN** the terminal stays drawn

#### Scenario: Returning to a terminal already seen
- **WHEN** the user leaves a terminal and comes back
- **THEN** it shows the last known screen while the next snapshot is fetched
- **AND** it does not show a spinner indefinitely

### Requirement: A second device is paired without copying a token by hand

omt MUST offer a way to add an instance that does not require transcribing a
credential. The token is long, it is secret, and reading it off one screen into
another is where people give up.

#### Scenario: Pairing from the terminal
- **WHEN** `omt web` starts
- **THEN** it prints a QR encoding the URL and its token

#### Scenario: The link carries the token in the fragment
- **WHEN** an invite URL is produced
- **THEN** the token is in the fragment, so it never reaches a server log

### Requirement: More than one instance

The client MUST hold several instances and switch between them, because one
person has a laptop and a desktop and both run omt.

#### Scenario: A second instance is added
- **WHEN** an instance is added while one is already known
- **THEN** both are listed and either can be opened

#### Scenario: An instance is forgotten
- **WHEN** an instance is removed
- **THEN** its credential is gone from this device

### Requirement: The keyboard survives a repaint

The terminal repaints continuously while a session runs. Repainting MUST NOT
detach the input, because detaching an element blurs it — which closes the
keyboard, abandons an in-progress IME composition, and stops dictation.

#### Scenario: The terminal repaints while somebody is typing
- **WHEN** snapshots arrive and the grid is redrawn
- **THEN** the input element is the same element as before
- **AND** it still holds focus

### Requirement: The grid fits the screen it is shown on

A pty starts 80 columns wide. The client MUST size it to what is actually
visible when a session is opened, not only when the window is resized.

#### Scenario: A session is opened on a narrow screen
- **WHEN** the terminal screen is shown
- **THEN** the session is resized to the columns and rows that fit
- **AND** output is not cut off beyond the right-hand edge

### Requirement: A session's state says what it is doing

A session MUST leave `starting` once its program has produced output. A state
that never changes is a label that carries no information.

#### Scenario: A shell prints its prompt
- **WHEN** a session's program writes to its pty
- **THEN** the session is reported as running rather than starting

### Requirement: An invite carries an address the other device can reach

The address in an invite MUST be one a browser elsewhere can open. A wildcard
bind means "every interface" to a listening socket and nothing at all to a
browser.

#### Scenario: The server is bound to a wildcard address
- **WHEN** an invite is produced for `0.0.0.0`
- **THEN** it names an address of this machine on the network

#### Scenario: The server is bound to loopback
- **WHEN** an invite is produced for a loopback address
- **THEN** omt says it will only work on this machine

### Requirement: A keystroke that goes nowhere says why

A write the instance refuses MUST be explained on the terminal screen. A
terminal that swallows keystrokes is indistinguishable from one that is broken.

#### Scenario: Another client holds the writer token
- **WHEN** somebody types into a session another client is driving
- **THEN** the screen says the write was refused and why
- **AND** the terminal remains readable

#### Scenario: The write then succeeds
- **WHEN** a later keystroke is accepted
- **THEN** the explanation is cleared


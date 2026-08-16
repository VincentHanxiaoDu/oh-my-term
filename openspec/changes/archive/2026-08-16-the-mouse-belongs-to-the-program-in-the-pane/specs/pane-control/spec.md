# pane-control Specification

## ADDED Requirements

### Requirement: A program that asked for the mouse receives it

When the program in a pane has enabled mouse tracking, omt SHALL forward mouse
events to it rather than acting on them, and SHALL encode each event in the
form that program asked for.

A multiplexer that swallows clicks so its own features can run has broken every
full-screen program its user relies on, and no host feature is worth that.

#### Scenario: A click in a pane whose program tracks the mouse

- **WHEN** the program has enabled mouse tracking and a button is pressed over
  its pane
- **THEN** omt forwards the event to that program
- **AND** omt does not begin a selection

#### Scenario: A program that asked for clicks is not sent motion

- **WHEN** the program enabled press-and-release tracking only
- **AND** the pointer moves across its pane with a button held
- **THEN** omt forwards the presses and releases and not the motion

#### Scenario: The encoding is the one the program asked for

- **WHEN** the program enabled the SGR encoding
- **THEN** events are forwarded in that encoding
- **AND** when it did not, they are forwarded in the legacy encoding

#### Scenario: What the user held down travels with the event

- **WHEN** a mouse event is forwarded and a modifier was held
- **THEN** the forwarded report says so

#### Scenario: A press is followed by its release

- **WHEN** a press has been forwarded to a program
- **THEN** the matching release is forwarded to that same program

### Requirement: The mouse can be taken back from the program

omt SHALL treat a mouse event held with Shift as its own, even where the
program in the pane has enabled tracking.

Every terminal omt runs inside implements this convention, so applications do
not expect shifted clicks — which is exactly what makes it available.

#### Scenario: Selecting inside a full-screen program

- **WHEN** a program has grabbed the mouse
- **AND** a drag is made with Shift held
- **THEN** omt makes a selection and forwards nothing

### Requirement: Where nothing grabbed the mouse, dragging selects

When no program in the pane has enabled mouse tracking, omt SHALL select text
by drag, in absolute scrollback rows, and place the selection on the clipboard
when the button is released.

Absolute rows are the same coordinates the keyboard selection uses; a
screen-relative selection moves under its own reader as soon as output arrives.

#### Scenario: Dragging across output

- **WHEN** a drag is made across a pane whose program has not grabbed the mouse
- **THEN** the text between the two points is selected

#### Scenario: Output arrives while a selection is held

- **WHEN** the pane produces output during or after a selection
- **THEN** the selection still covers the same text

#### Scenario: A click that never moved

- **WHEN** a button is pressed and released without moving
- **THEN** nothing is selected and nothing is put on the clipboard

### Requirement: The wheel goes where the pane's state says

The wheel SHALL be reported to a program that enabled mouse tracking,
translated to arrow keys only when the alternate screen is active **and**
alternate scroll was enabled, and otherwise scroll omt's own scrollback.

Translating for every alternate-screen program sends arrow keys to programs
that never asked for them, which in an editor is a cursor that moves when
somebody scrolls.

#### Scenario: A program tracking the mouse

- **WHEN** the wheel turns over a pane whose program enabled tracking
- **THEN** a wheel report is forwarded to it

#### Scenario: A pager that asked for alternate scroll

- **WHEN** the alternate screen is active and alternate scroll was enabled
- **THEN** the wheel is forwarded as arrow keys

#### Scenario: A full-screen program that asked for neither

- **WHEN** the alternate screen is active and alternate scroll was not enabled
- **THEN** no arrow keys are sent

#### Scenario: An ordinary shell

- **WHEN** the wheel turns over a pane with no tracking and no alternate screen
- **THEN** omt scrolls its own scrollback

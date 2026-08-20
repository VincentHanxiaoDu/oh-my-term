# pane-control Specification

## Purpose
TBD - created by archiving change match-their-key-surface. Update Purpose after archive.

## Requirements

### Requirement: One pane can fill the screen and come back

omt SHALL be able to expand the focused pane to the whole area and restore the
arrangement exactly.

The most-used key in every multiplexer, because splitting is for comparing and
zooming is for working, and both happen in the same minute.

#### Scenario: Zooming

- **WHEN** somebody zooms a pane in a split arrangement
- **THEN** that pane has the whole area and the others are not drawn

#### Scenario: Coming back

- **WHEN** they zoom out
- **THEN** the arrangement is exactly what it was, including the ratios

#### Scenario: Zooming with one pane

- **WHEN** there is nothing to zoom over
- **THEN** nothing changes, and nothing is said

### Requirement: Focus can return to where it just was

omt SHALL be able to move focus to the previously focused pane.

Moving between two panes by direction means thinking about which direction;
this is how everybody actually moves between two things.

#### Scenario: Going back

- **WHEN** focus moves from one pane to another and then back
- **THEN** it lands on the first

#### Scenario: Nowhere to go back to

- **WHEN** focus has never moved
- **THEN** nothing changes

### Requirement: A pane can be moved rather than the focus

omt SHALL be able to swap the focused pane with its neighbour in a direction.

#### Scenario: Swapping

- **WHEN** a pane is swapped with the one to its right
- **THEN** the two exchange places and focus follows the pane

#### Scenario: Nothing in that direction

- **WHEN** there is no neighbour that way
- **THEN** the arrangement is unchanged

### Requirement: A session can be given a name

A session SHALL be renameable, and the name SHALL be what every surface shows.

A roster of six things called `shell` is what omt shows today. Detection
answers it for agents; for everything else, the answer is letting somebody say
what a thing is.

#### Scenario: Naming one

- **WHEN** a session is renamed
- **THEN** the terminal, the panel and the web client all show the new name

#### Scenario: An empty name

- **WHEN** the name given is empty
- **THEN** it is refused, rather than leaving a row with nothing in it

### Requirement: Focus can jump to the next agent that wants something

omt SHALL be able to move focus to the next session whose agent needs a person,
without opening a list.

This is the product's subject. Needing a list and a choice to reach it is two
steps for the thing omt exists to make one.

#### Scenario: Something is waiting

- **WHEN** an agent is blocked and somebody presses the key
- **THEN** focus moves to it

#### Scenario: Several are waiting

- **WHEN** the key is pressed again
- **THEN** focus moves to the next one, and wraps

#### Scenario: Nothing is waiting

- **WHEN** no agent needs anybody
- **THEN** focus does not move, and omt says why rather than appearing stuck

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

### Requirement: A layout follows the window it is drawn in

When the terminal's size changes, the next frame SHALL be laid out at the new
size, whatever the source of the change.

The window could move under omt without its knowledge: `SIGWINCH` reaches the
pty's foreground process group, which is the shell in the pane, not omt — so
a shrunken window kept its old geometry and drew panes nobody could see.

#### Scenario: The window shrinks

- **WHEN** the terminal shrinks while a split is on screen
- **THEN** the next frames re-lay the panes at the new size, both visible

### Requirement: A pane exists while a prompt fits

A split SHALL survive a shrink down to twelve columns per pane, and a pane
denied room shall not be drawn over its neighbour.

Forty columns — the width at which `git log --oneline` looks good — made a
sixty-column window unable to hold a split: one pane took everything and the
other, still painted, overwrote the live screen with its stale grid.

#### Scenario: No room

- **WHEN** a pane's rect comes out zero
- **THEN** it is not painted, and content on screen is left alone

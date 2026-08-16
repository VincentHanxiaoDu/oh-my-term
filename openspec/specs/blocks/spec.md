# blocks Specification

## Purpose
TBD - created by archiving change other terminals-parity-blocks. Update Purpose after archive.
## Requirements
### Requirement: Output with no open block still produces one

Output arriving when no block is open MUST open one rather than being dropped.
A shell that emits no OSC 133 — or emits it late, or is `ssh` on the far side
of a connection that has none — must still produce something a surface can
show, or every one of those sessions appears empty.

#### Scenario: A shell with no shell integration at all
- **WHEN** output arrives and no block has ever been opened
- **THEN** a block is opened covering that output
- **AND** it is marked as having no command attributed to it

#### Scenario: The marks arrive later
- **WHEN** an early block exists and a prompt mark then arrives
- **THEN** the early block is closed at that row
- **AND** a new block opens for the marked command

#### Scenario: A marked session is unaffected
- **WHEN** output arrives while a marked block is open
- **THEN** no early block is created

### Requirement: An unattributed block says so

A block opened from output alone MUST be distinguishable from one whose command
the shell reported. A surface offering "re-run this" must not offer it for a
block whose command omt never saw.

#### Scenario: A surface asks what it can re-run
- **WHEN** a block was opened from output with no command mark
- **THEN** it reports that it has no command
- **AND** its command text is empty rather than guessed from the screen

### Requirement: A workspace can open as a layout

A launch configuration MUST describe a recursive split of panes with the command
each runs, and opening it MUST produce that layout.

#### Scenario: A two-pane configuration
- **WHEN** a launch configuration describing two panes is opened
- **THEN** two sessions start with the commands it names
- **AND** both are in the workspace's view

#### Scenario: A configuration naming a directory that is gone
- **WHEN** a pane names a working directory that does not exist
- **THEN** opening reports which pane failed rather than silently starting it elsewhere

### Requirement: A command line is highlighted from one table

Highlighting MUST classify tokens from the same table that names commands, so
that a token highlighted as a command and a token completed as a command are
the same set.

#### Scenario: A command line with a flag and a path
- **WHEN** `cargo test --workspace crates/omt` is classified
- **THEN** `cargo` is a command, `test` a subcommand, `--workspace` a flag, and the rest an argument

#### Scenario: A quoted string containing a flag
- **WHEN** a quoted argument contains something that looks like a flag
- **THEN** it is classified as part of the string rather than as a flag

### Requirement: An image a program draws is seen

omt SHALL recognise the kitty graphics sequence and the iTerm2 inline-image
sequence in a pane's output, including their chunked forms.

An APC string is consumed by the parser without any callback, so a graphics
sequence is invisible rather than unhandled — and the output after it lands
where the image should be, which makes a program that drew an image look like
a program that did nothing.

#### Scenario: A kitty graphics transmission

- **WHEN** a program writes a kitty graphics sequence to its pane
- **THEN** omt records that an image arrived, and its size in cells where the
  sequence says

#### Scenario: An image sent in pieces

- **WHEN** the transmission is split across continuation chunks
- **THEN** omt treats it as one image, once it is complete

#### Scenario: A transmission that never completes

- **WHEN** a chunked transmission stops before its final chunk
- **THEN** no image is recorded from the part that arrived

#### Scenario: An image larger than omt will hold

- **WHEN** a transmission exceeds the bound omt keeps for one image
- **THEN** the whole transmission is dropped rather than rendered from part of
  itself
- **AND** omt keeps reading the pane's output normally afterwards

#### Scenario: An escape sequence that is not graphics

- **WHEN** a pane emits an APC string that is not a graphics sequence
- **THEN** it is consumed, and nothing is drawn from it

### Requirement: An image occupies the cells it needs

An image SHALL reserve the rows it covers, so output written after it is not
placed on top of it.

An image that occupies nothing is one the next line overwrites, which is
indistinguishable from the image never having arrived.

#### Scenario: Output after an image

- **WHEN** a program draws an image and then writes more lines
- **THEN** those lines appear after the image's rows, not over them

#### Scenario: Scrolling past an image

- **WHEN** the pane scrolls far enough that the image leaves the screen
- **THEN** the rows it occupied scroll with it, like any other content

### Requirement: What arrived is always shown as something

Where omt cannot draw the image itself, it SHALL show a card in the reserved
cells naming what arrived.

Blankness is the one outcome that cannot be told apart from a failure. A card
says an image is there, which is what lets somebody reach it another way.

#### Scenario: An image omt cannot draw

- **WHEN** an image has been recorded and omt cannot render it
- **THEN** the reserved rows show what it is and how to reach it


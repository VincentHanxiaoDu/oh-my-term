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

### Requirement: An image is drawn only where the terminal confirmed it can

omt SHALL ask the outer terminal whether it supports the graphics protocol,
before reading any input, and SHALL draw an image only where that terminal
answered.

Assuming support from a terminal's name gives a feature that silently does
nothing, and a user who cannot tell whether they mis-typed or omt mis-guessed.
Asking mid-session is worse: the reply arrives in the same stream as
keystrokes, and an answer mistaken for typing puts escape text into somebody's
shell.

#### Scenario: A terminal that answers

- **WHEN** the outer terminal replies to the handshake
- **THEN** images are drawn

#### Scenario: A terminal that says nothing

- **WHEN** no reply arrives before the handshake's deadline
- **THEN** no graphics sequences are ever emitted
- **AND** the image's rows show its card, exactly as before

#### Scenario: A reply that is not the one asked for

- **WHEN** something else arrives on the descriptor during the handshake
- **THEN** it is not treated as support

### Requirement: What is drawn is clipped to the pane it belongs to

An image SHALL be drawn within its pane's rectangle, and the part of it outside
that rectangle SHALL NOT be drawn.

A pane is one region of somebody's screen. An image that spilled past it would
be drawn over another session's output, which is worse than not drawing it.

#### Scenario: An image taller than the space left in the pane

- **WHEN** the image's rows extend past the bottom of the pane
- **THEN** only the rows inside the pane are drawn

#### Scenario: An image scrolled partly out of view

- **WHEN** the image's first rows have scrolled above the pane's top
- **THEN** the visible part is drawn in the right place

#### Scenario: An image scrolled entirely out of view

- **WHEN** none of the image's rows are in the pane
- **THEN** nothing is drawn for it

#### Scenario: The size of a cell is not known

- **WHEN** the outer terminal did not report its cell size in pixels
- **THEN** no image is drawn, because clipping cannot be worked out

### Requirement: The card stays underneath the image

The rows an image reserved SHALL keep their card, and the image SHALL be drawn
over them.

The fallback becomes structural rather than conditional: if the image does not
arrive — evicted, refused, a terminal that stopped answering — what was already
going to be on those rows is.

#### Scenario: An image whose bytes are no longer held

- **WHEN** the payload was evicted to stay within omt's bound
- **THEN** the card is what shows

### Requirement: An image is re-sent only when its placement changed

omt SHALL re-send an image to the outer terminal when its position, its
clipping or its pane changed, and SHALL NOT re-send it on every frame.

A frame is drawn many times a second and an image is measured in megabytes.
Re-sending it each time is a link that never goes idle.

#### Scenario: A frame in which nothing moved

- **WHEN** the pane redraws and the image's placement is unchanged
- **THEN** nothing is sent for it

#### Scenario: The pane scrolls

- **WHEN** the image moves to a different row of the screen
- **THEN** it is sent again at its new place

### Requirement: omt installs the shell snippet it documents

When `shell.auto_install` is on, omt SHALL add its command-mark snippet to the
shell's configuration file, once, inside a marked block.

The setting has been on by default since it was written and nothing read it, so
a default installation was promised marks and got the heuristic fallback — a
slightly worse experience rather than an error, which is why nobody reported
it.

#### Scenario: A shell with no marks

- **WHEN** omt starts a shell it has a snippet for and the file has no marker
- **THEN** the snippet is added inside a marked block, and the change is named
  on the way in

#### Scenario: A file that already has it

- **WHEN** the file already contains omt's marker
- **THEN** the file is left byte for byte as it was

#### Scenario: The setting is off

- **WHEN** `shell.auto_install` is `false`
- **THEN** nothing is written, and `shell.integration` still reports the line
  to add

#### Scenario: A shell omt has no snippet for

- **WHEN** the shell is not one omt has a snippet for
- **THEN** nothing is written, and no other shell's line is used

### Requirement: The configured word separators end a word

Copy mode's word motion SHALL treat `terminal.word_separators` as ending a
word.

The setting is in the settings reference and in the importer, so somebody
moving from another terminal had it carried across and then ignored: a word
motion stopped only at whitespace, which makes a path one word.

#### Scenario: A path under the default separators

- **WHEN** the separators include `/` and a word motion runs over a path
- **THEN** it stops at the separator

#### Scenario: Nothing configured

- **WHEN** no separators are configured
- **THEN** a word ends at whitespace, as it did before

### Requirement: The configured block cap is the cap

A session SHALL retain at most `terminal.blocks.max_retained` command blocks.

#### Scenario: A lowered cap

- **WHEN** the cap is lower than the built-in one
- **THEN** the session keeps that many blocks

### Requirement: Blocks can be turned off

When `terminal.blocks.enabled` is `false`, no command marks SHALL open or close
a block, and output SHALL NOT open unattributed blocks.

The setting changed nothing: the tracker was always on, and the fallback that
makes an unmarked session legible is itself a block.

#### Scenario: Disabled

- **WHEN** blocks are disabled and a marked command runs
- **THEN** no block exists afterwards

#### Scenario: Enabled, the default

- **WHEN** nothing is said
- **THEN** marks work and unmarked output opens an unattributed block, as
  before


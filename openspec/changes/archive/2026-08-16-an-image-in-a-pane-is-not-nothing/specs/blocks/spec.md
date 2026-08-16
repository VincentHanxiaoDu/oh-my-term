# blocks Specification

## ADDED Requirements

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

# blocks Specification

## ADDED Requirements

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

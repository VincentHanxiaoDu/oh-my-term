# onboarding Specification

## ADDED Requirements

### Requirement: omt's own chrome is coloured from the theme

The status bar, panels, separators and mode hints SHALL take their colours
from the theme, and SHALL fall back to reverse video where the terminal offers
no colour.

Reverse video across a whole row is a solid block of the foreground colour: on
a dark theme the bar is a white bar. The theme has existed the whole time and
the chrome never read it.

#### Scenario: A terminal with colour

- **WHEN** the terminal reports that it supports colour
- **THEN** the bar and panels are drawn on a surface derived from the theme's
  background

#### Scenario: A terminal without colour

- **WHEN** the terminal reports no colour, or the user asked for none
- **THEN** the chrome is drawn exactly as it was before, in reverse video

#### Scenario: Nothing is said only in colour

- **WHEN** the chrome reports a state
- **THEN** that state is legible without colour

### Requirement: A panel is a panel, not a band

A centred panel SHALL paint only from its own edge, leaving the margin in
front of it untouched.

Painting the margin turns a floating panel into a full-width band with a box
drawn inside it, which is not the shape being drawn.

#### Scenario: A panel narrower than the screen

- **WHEN** a panel is drawn on a screen wider than it is
- **THEN** the columns beside it keep whatever was already there

## ADDED Requirements

### Requirement: A terminal that closes says why it closed

When omt exits because its last session ended, it SHALL say so, name how that
session's program ended, and report an exit status above 128 as the signal it
stands for.

A terminal that closes without a word is indistinguishable from one that
crashed, and gets reported as one.

#### Scenario: The last session's program was signalled

- **WHEN** the last session's program ended with a status above 128
- **THEN** omt names the signal and the status

#### Scenario: The last session's program exited normally

- **WHEN** the last session's program exited with zero
- **THEN** omt says the session ended, without inventing a fault

#### Scenario: The message outlives the screen

- **WHEN** omt says why it closed
- **THEN** it does so after the alternate screen has been given back

# web-client Specification

## Purpose
TBD - created by archiving change web-client-rebuild. Update Purpose after archive.
## Requirements
### Requirement: A screen shows one state at a time

Every screen MUST render exactly one of loading, empty, error or content. Two
statements about the same thing on one screen means one of them is wrong, and
the user cannot tell which.

#### Scenario: A list that is still loading
- **WHEN** a screen's data has been requested and has not arrived
- **THEN** it shows a loading state
- **AND** it does not simultaneously show an empty state

#### Scenario: A list that came back empty
- **WHEN** data arrives and is empty
- **THEN** it shows one empty state saying why, not a count and a contradiction

#### Scenario: A header agrees with its body
- **WHEN** a screen reports a count in its header
- **THEN** that count is derived from what the body renders

### Requirement: A tap opens what the session is

Tapping a session MUST open the screen that matches what it is. A shell has no
agent threads, and routing it to a thread grid shows an empty screen for the one
thing the user opened it for.

#### Scenario: A plain shell
- **WHEN** a session with no agent is tapped
- **THEN** its terminal opens

#### Scenario: A session running an agent
- **WHEN** a session with agent threads is tapped
- **THEN** its threads open

### Requirement: Navigation has a way back

Every screen except the roster MUST offer a way back, and the browser's own back
gesture MUST work.

#### Scenario: A screen below the roster
- **WHEN** any screen other than the roster is showing
- **THEN** a back affordance is present

#### Scenario: The device's back gesture
- **WHEN** the user triggers the browser's back
- **THEN** the client returns to the previous screen rather than leaving the app

### Requirement: An error belongs to what caused it and clears

A failed call MUST be reported against the thing that failed and MUST clear when
that thing succeeds. Errors MUST NOT accumulate as text under unrelated content.

#### Scenario: A call fails
- **WHEN** a capability call fails for a screen
- **THEN** that screen shows the error in place of its content

#### Scenario: A retry succeeds
- **WHEN** the same call later succeeds
- **THEN** the error is gone

#### Scenario: An unrelated screen
- **WHEN** one screen's call fails
- **THEN** other screens do not show that error

### Requirement: Presentation comes from tokens

Type sizes, spacing, colour roles and elevation MUST be defined once and
referenced, so that a screen is composed rather than hand-styled and two screens
cannot disagree about what a heading is.

#### Scenario: A new screen is added
- **WHEN** a screen is built from the system's components
- **THEN** it inherits the type scale, spacing and colours without new values

#### Scenario: Touch targets
- **WHEN** any interactive element is rendered
- **THEN** it is at least 44 CSS pixels on its smallest side


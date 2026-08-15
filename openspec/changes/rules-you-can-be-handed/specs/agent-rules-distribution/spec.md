## ADDED Requirements

### Requirement: A rule set is captured from a real screen

omt SHALL be able to write out what a pane is drawing, with each named region
marked, in a form somebody can edit into rules.

Rules written from memory are wrong in ways that only a real screen finds: the
shipped rule for Claude Code's permission prompt was written from memory and
was wrong about the first screen anybody sees.

#### Scenario: Capturing a pane

- **WHEN** somebody captures a pane that has an agent in it
- **THEN** the file holds the visible screen, and shows what each region
  contains
- **AND** it names the agent omt believes is there

#### Scenario: A pane with nothing interesting in it

- **WHEN** the pane is a plain shell
- **THEN** the capture still works, and says no agent was identified

### Requirement: A rule set is never installed without being asked for

omt SHALL NOT fetch, update or apply a rule set except when a person asks it
to, in that moment.

A rule set is a matcher that runs against the screen. Fetching one on a timer
means running a stranger's matcher over somebody's work without being asked,
which is a different thing from updating a dependency.

#### Scenario: Starting up

- **WHEN** omt starts
- **THEN** it does not check for, fetch, or apply any rule set

#### Scenario: Importing one

- **WHEN** somebody imports a rule set
- **THEN** omt says what it would change before changing anything
- **AND** installs it only after that is confirmed

### Requirement: An import is checked before it is kept

An imported rule set SHALL be refused unless it parses, names the agent it is
being installed for, and contains no rule without a test.

#### Scenario: A set for a different agent

- **WHEN** the file describes one agent and is being installed for another
- **THEN** it is refused, and the two names are said

#### Scenario: A rule that matches everything

- **WHEN** a rule has no test
- **THEN** the set is refused, naming that rule

#### Scenario: A set that needs a newer omt

- **WHEN** the set declares an engine this build does not have
- **THEN** it is refused whole, rather than partly applied

### Requirement: A verdict can be traced to a rule

omt SHALL be able to say which rule decided a pane's state, and on what.

A wrong answer that cannot be traced is one people argue about.

#### Scenario: Asking why

- **WHEN** somebody asks what the rules make of a pane
- **THEN** omt names the rule that won, the region it looked at, and the state
  it concluded

#### Scenario: Nothing matched

- **WHEN** no rule matches
- **THEN** omt says so, rather than naming a rule that did not fire

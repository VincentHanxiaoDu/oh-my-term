## MODIFIED Requirements

### Requirement: Quitting says what it is about to end

omt SHALL leave a terminal's sessions running when the terminal quits, and SHALL
ask before ending anything it is about to end on purpose.

The promise a multiplexer exists to make. What was true before this change is
that omt asked honestly about killing the work; what is true after it is that
quitting does not kill the work.

#### Scenario: Quitting with work in progress

- **WHEN** the quit key is pressed while an agent is mid-turn
- **THEN** the terminal exits
- **AND** the agent keeps working

#### Scenario: Coming back

- **WHEN** omt is started again in the same place
- **THEN** it finds the sessions that were left
- **AND** each pane shows what its session's screen shows now

#### Scenario: Ending a session on purpose

- **WHEN** somebody closes a session rather than the terminal
- **THEN** that session ends, and only that one

### Requirement: The key says what it does

omt SHALL label this key `detach` once quitting leaves the sessions running.

`detach` is a promise that the session survives. It became the wrong word when
the terminal owned the ptys, and it becomes the right one again here.

#### Scenario: The key strip

- **WHEN** the leader strip is shown
- **THEN** the label is `detach`

### Requirement: A terminal starts what it needs

omt SHALL find a running instance to attach to, and SHALL start one when there
is none.

Nobody should have to know that a daemon exists to keep their agent alive.

#### Scenario: Nothing running yet

- **WHEN** omt starts and no instance is reachable
- **THEN** one is started, and the terminal attaches to it

#### Scenario: Something already running

- **WHEN** an instance is reachable
- **THEN** the terminal attaches to it rather than starting a second

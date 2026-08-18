# agent-panel Specification

## ADDED Requirements

### Requirement: The agent markers can be declined

When `shell.inject_env` is off, no pane SHALL carry `OMT_SESSION`,
`OMT_SOCK` or `OMT_AGENT_EVENTS`.

The switch was decoration with a wrong default: panes always carried them and
the settings screen reported the value nothing consulted.

#### Scenario: Off

- **WHEN** `shell.inject_env` is `false` and a pane is started
- **THEN** it carries none of the markers

#### Scenario: On, the default

- **WHEN** nothing is said
- **THEN** panes carry all three, as they always have

### Requirement: OSC 133 is one decision with two halves

`shell.osc133` SHALL decide both halves: `emit`/`both` install the emitter,
`consume`/`both` read the marks. `consume` alone means marks are read and
never installed; `emit` alone the reverse.

#### Scenario: Consume only

- **WHEN** the setting is `consume`
- **THEN** the block tracker reads marks and the installer installs nothing

### Requirement: Detection can be declined

When `agents.detection` is off, omt SHALL not observe panes for agents and
SHALL not advertise a detected list.

#### Scenario: Off

- **WHEN** the setting is `false`
- **THEN** no observations are made and the first-run list is empty, while
  the explicit install flow still works

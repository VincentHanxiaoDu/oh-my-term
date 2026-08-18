# onboarding Specification

## ADDED Requirements

### Requirement: The terminal modes omt asks for are configurable

omt SHALL ask for mouse reporting and bracketed paste only when the
configuration allows them.

Both were asked for unconditionally, so somebody who turned the mouse off still
had every click taken from the terminal underneath — which is the one thing
that setting exists to allow.

#### Scenario: The mouse turned off

- **WHEN** `terminal.mouse_reporting` is `false`
- **THEN** omt does not ask the terminal to report the mouse

### Requirement: The hint line can be turned off

When `hints.enabled` is `false`, omt SHALL NOT draw the hint line.

#### Scenario: Hints off

- **WHEN** hints are off
- **THEN** no hint is drawn

#### Scenario: The leader strip

- **WHEN** hints are off and the leader is pressed
- **THEN** the strip still appears, because it is a menu that was asked for
  rather than a hint that was offered

### Requirement: The web surface listens where the configuration says

`omt web` with no address SHALL use `server.bind`.

#### Scenario: A configured address

- **WHEN** `server.bind` is set and `omt web` is run with no `--bind`
- **THEN** it listens on the configured address

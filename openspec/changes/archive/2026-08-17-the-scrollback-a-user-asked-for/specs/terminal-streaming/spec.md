# terminal-streaming Specification

## ADDED Requirements

### Requirement: The configured scrollback is the scrollback

A session SHALL keep the number of scrollback lines the configuration asks for.

It kept the built-in ten thousand whatever the setting said, while the settings
screen reported the user's number and the file it came from — which is a
setting that is accepted, listed, traced, and ignored.

#### Scenario: A configured cap

- **WHEN** `terminal.scrollback_lines` is set and a session is started
- **THEN** that session keeps that many lines

#### Scenario: No setting

- **WHEN** nothing is set
- **THEN** the built-in default applies, and omt starts normally

### Requirement: The configured shell is preferred

A new pane SHALL start `terminal.default_shell` when it is set, and `$SHELL`
otherwise.

#### Scenario: A configured shell

- **WHEN** `terminal.default_shell` names a shell
- **THEN** a new pane runs it

#### Scenario: An empty setting

- **WHEN** the setting is empty or blank
- **THEN** it is ignored rather than run as a program with no name

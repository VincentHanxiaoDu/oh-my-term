# onboarding Specification

## Purpose
TBD - created by archiving change zero-config-defaults. Update Purpose after archive.
## Requirements
### Requirement: omt reports whether the shell marks its commands

omt MUST be able to say whether the current shell emits shell-integration marks,
and MUST be able to produce the line that makes it, for the shell in use.

#### Scenario: A shell that already emits the marks
- **WHEN** the integration status is asked for and marks have been seen
- **THEN** it reports that integration is active
- **AND** offers no snippet, because there is nothing to install

#### Scenario: A shell that does not
- **WHEN** the integration status is asked for and no marks have been seen
- **THEN** it reports that integration is absent
- **AND** returns a snippet for that shell, naming the file it belongs in

#### Scenario: A shell omt does not know
- **WHEN** the shell is one omt has no snippet for
- **THEN** it says so rather than returning a snippet for a different shell

### Requirement: omt never edits the user's shell configuration

omt MUST NOT write to a user's shell rc file. The snippet is printed and the
user installs it.

#### Scenario: The snippet is requested
- **WHEN** a caller asks for the shell-integration snippet
- **THEN** the snippet is returned as text
- **AND** no file outside omt's own configuration directory is modified

### Requirement: A first run says what omt found

On a machine with no omt configuration, starting omt MUST print a short summary
of what it detected and the keys worth knowing, exactly once.

#### Scenario: The first ever run
- **WHEN** omt starts and no configuration directory exists
- **THEN** it prints what it detected and continues into the session
- **AND** it asks no question and waits for no input

#### Scenario: Every run after that
- **WHEN** omt starts and its configuration directory exists
- **THEN** it prints no summary

### Requirement: Every setting is discoverable without knowing its name

omt MUST be able to report the full set of settings — name, type, default and a
one-line description — from the same declarations that resolve their values.

#### Scenario: A client builds a settings screen
- **WHEN** the configuration schema is requested
- **THEN** every setting is listed with its default and description
- **AND** the list is derived from the declarations, so a setting cannot exist
  without appearing in it


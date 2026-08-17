# blocks Specification

## ADDED Requirements

### Requirement: omt installs the shell snippet it documents

When `shell.auto_install` is on, omt SHALL add its command-mark snippet to the
shell's configuration file, once, inside a marked block.

The setting has been on by default since it was written and nothing read it, so
a default installation was promised marks and got the heuristic fallback — a
slightly worse experience rather than an error, which is why nobody reported
it.

#### Scenario: A shell with no marks

- **WHEN** omt starts a shell it has a snippet for and the file has no marker
- **THEN** the snippet is added inside a marked block, and the change is named
  on the way in

#### Scenario: A file that already has it

- **WHEN** the file already contains omt's marker
- **THEN** the file is left byte for byte as it was

#### Scenario: The setting is off

- **WHEN** `shell.auto_install` is `false`
- **THEN** nothing is written, and `shell.integration` still reports the line
  to add

#### Scenario: A shell omt has no snippet for

- **WHEN** the shell is not one omt has a snippet for
- **THEN** nothing is written, and no other shell's line is used

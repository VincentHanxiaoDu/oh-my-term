# repository-gates Specification

## ADDED Requirements

### Requirement: A gate run runs every gate it was named

`xtask` SHALL run every task named on its command line, in the order given, and
SHALL NOT ignore any word of it.

A runner that reads only the first name and exits zero reports four gates as
passing when they did not run. That is indistinguishable from four passes to
everyone who reads the output.

#### Scenario: Several gates in one command

- **WHEN** `cargo run -p xtask -- layering licensing file-length codegen-check test-isolation`
  is run
- **THEN** all five gates run, each announcing itself by name before it runs

#### Scenario: A name the runner does not know

- **WHEN** a word on the command line is not a task name and is not a known
  option
- **THEN** the run fails, naming the word, before any gate runs

### Requirement: A failing gate does not hide the others

When several gates are named, a failure SHALL NOT stop the remaining gates, and
the process SHALL exit non-zero naming every gate that failed.

The reason to name five gates is to learn about five; stopping at the first
failure turns one run into five.

#### Scenario: One gate fails and others follow it

- **WHEN** a gate in the middle of the list fails
- **THEN** the gates after it still run, and the exit status is non-zero with
  every failed gate named

### Requirement: An option is spelled as an option

A gate's own argument SHALL be spelled as an option rather than as a bare word.

The licensing gate took the second bare word as a directory to check. On a line
naming several gates, that word is the next gate's name — so the gate silently
checked a path that does not exist.

#### Scenario: The licensing gate pointed at a fixture

- **WHEN** the licensing gate is given `--root <path>`
- **THEN** it checks that directory instead of the repository root

#### Scenario: A bare word after a task name

- **WHEN** a bare word follows a task name
- **THEN** it is treated as another task name, and rejected if it is not one

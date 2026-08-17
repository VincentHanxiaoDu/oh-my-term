# keybindings Specification

## ADDED Requirements

### Requirement: The binding file is opened

omt SHALL read `keybindings.toml` from the layers
`docs/architecture/10-configuration.md` §2.1 defines, and SHALL apply what it
finds.

The file was specified, described to users and validated by a capability, and
no code opened it. A rebind that does nothing is worse than a rebind that is
refused: nothing to read, nothing to grep, and a user who concludes their
keyboard is broken.

#### Scenario: A user binding

- **WHEN** `keybindings.toml` binds a trigger to a capability
- **THEN** that trigger invokes that capability, and `keys.list` reports the
  binding with the file it came from

#### Scenario: No file at all

- **WHEN** no binding file exists in any layer
- **THEN** the resolved map is exactly the defaults

#### Scenario: A malformed file

- **WHEN** the file exists and is not valid TOML
- **THEN** omt refuses to start rather than starting with different keys

### Requirement: The leader is what the file says

The leader chord and the actions reached through it SHALL come from the
resolved map.

A constant and a hard-coded match make `leader = "ctrl-a"` unreachable while the
documentation says it is one edit.

#### Scenario: A relocated leader

- **WHEN** the file sets `leader = "ctrl-a"`
- **THEN** that chord opens the leader namespace and `ctrl-b` reaches the
  program in the pane

### Requirement: The keymap flavour comes from the binding file

The copy-mode flavour SHALL be read from the binding file's `keymap` key.

It was read from the configuration stack, where no such key exists: the merge
answers `OMT-C101 unknown key`, so the flavour was always the default and the
user's setting produced an error.

#### Scenario: emacs motions

- **WHEN** the binding file sets `keymap = "emacs"`
- **THEN** copy mode uses emacs motions, and no unknown-key diagnostic is
  produced

### Requirement: Every binding that is not installed says so

A binding refused for a reserved chord, an unmet platform or terminal gate, or
a mode declared under a non-modal keymap SHALL be reported with its code and a
sentence naming what to do.

#### Scenario: A reserved chord without force

- **WHEN** a global binding names a chord on the refusal list without
  `force = true`
- **THEN** it is refused as `OMT-C410`, naming the chord and why it is reserved

#### Scenario: An unmet gate

- **WHEN** a user binding declares a `platform` or `requires` that is not met
- **THEN** it is not installed, and is reported as `OMT-C408`

### Requirement: A `when` predicate is over a known vocabulary

`when` SHALL be a boolean expression over
`docs/architecture/16-input-and-keymap.md` §4.1's context names, and an unknown
name SHALL be an error.

A predicate that silently never holds is a binding that silently never fires.

#### Scenario: A predicate that is not satisfied

- **WHEN** a binding's `when` does not hold for the current contexts
- **THEN** the key falls through to the next binding, and to passthrough if
  none matches

#### Scenario: A name omt does not have

- **WHEN** `when` names a context that is not in the set
- **THEN** the file reports it, naming the unknown word

### Requirement: Resolution order is the specified one

Bindings SHALL resolve by pending chord, then modal context, then specificity,
then config layer, and SHALL pass through when nothing matches.

#### Scenario: A more specific binding wins

- **WHEN** two bindings match one chord and one has more context terms
- **THEN** the one with more terms is chosen

#### Scenario: Nothing matches

- **WHEN** no binding matches a key in any layer
- **THEN** the original bytes reach the program in the pane, unmodified

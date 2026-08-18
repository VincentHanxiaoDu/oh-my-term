# blocks Specification

## ADDED Requirements

### Requirement: The configured word separators end a word

Copy mode's word motion SHALL treat `terminal.word_separators` as ending a
word.

The setting is in the settings reference and in the importer, so somebody
moving from another terminal had it carried across and then ignored: a word
motion stopped only at whitespace, which makes a path one word.

#### Scenario: A path under the default separators

- **WHEN** the separators include `/` and a word motion runs over a path
- **THEN** it stops at the separator

#### Scenario: Nothing configured

- **WHEN** no separators are configured
- **THEN** a word ends at whitespace, as it did before

### Requirement: The configured block cap is the cap

A session SHALL retain at most `terminal.blocks.max_retained` command blocks.

#### Scenario: A lowered cap

- **WHEN** the cap is lower than the built-in one
- **THEN** the session keeps that many blocks

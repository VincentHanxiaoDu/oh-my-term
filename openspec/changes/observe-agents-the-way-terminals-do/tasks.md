## 1. Stop the tests lying

- [x] 1.1 The screen harness strips every inherited agent environment variable
- [x] 1.2 A test that a plain shell reports no agent, which the contaminated
      harness could never have failed
- [x] 1.3 The three tests that passed on inherited environment are re-run and
      shown to fail before anything else is fixed

## 2. The OSC marker tier

- [x] 2.1 `SetTitle` reaches the merge machine as a reading
- [x] 2.2 Identity from the title: a per-agent title pattern
- [x] 2.3 State from the title's leading glyph, per agent
- [ ] 2.4 OSC progress as a second marker reading
- [x] 2.5 The tier sits above `Heuristic` and below `Hook`
- [x] 2.6 A title that names nothing yields no reading rather than a guess
- [x] 2.7 The glyph never reaches a person

## 3. The process probe, repaired

- [x] 3.1 A process name is read without argv, through the interface that needs
      no permission
- [x] 3.2 `identify` reports a name-only match rather than nothing
- [x] 3.3 A test that asks each layer of the probe directly — foreground
      process, process info, both names, environment, `identify` — because
      "does the bar say claude" is two layers above the question and told
      nobody which layer had failed
- [x] 3.4 The env-marker path keeps working where the environment is readable

## 4. Screen regions as data

- [ ] 4.1 Named regions over the pane's grid: the whole recent screen, the last
      N non-empty lines, after the last horizontal rule, the prompt box body,
      the OSC title
- [ ] 4.2 Rules with a state, a priority, and `contains` / `regex` /
      `line_regex`
- [ ] 4.3 `any` / `all` / `not` combinators
- [ ] 4.4 A rule that suppresses state updates while an agent shows its own
      history
- [ ] 4.5 Rules are data with a version, not code
- [ ] 4.6 Claude Code's rule set, from its observed interface
- [ ] 4.7 A rule set for each other adapter omt already declares
- [ ] 4.8 The floor still applies when no rule matches

## 5. What a person is told

- [ ] 5.1 Done and unseen is distinct from idle and seen
- [ ] 5.2 Focusing a pane marks it seen
- [x] 5.3 No surface prints "unknown"
- [x] 5.4 The agent's name is the first thing on its row
- [x] 5.5 The evidence is still available, after the verdict

## 6. Both surfaces agree

- [ ] 6.1 The web client shows the same state, from the same readings
- [ ] 6.2 Done-and-unseen reaches the phone
- [ ] 6.3 A test that drives both against one instance and compares

## 7. Close-out

- [ ] 7.1 clippy, workspace tests, layering, codegen
- [ ] 7.2 `npm run check`
- [ ] 7.3 The reported sequence, end to end: omt, then the real agent, then ask
      omt what is running — in a harness with no inherited agent environment

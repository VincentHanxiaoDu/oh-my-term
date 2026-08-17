# Tasks

## 1. The context set and its predicates
- [x] 1.1 `ContextSet` with 16 §4.1's names, exactly
- [x] 1.2 `Predicate::parse` — `&&`, `||`, `!`, parentheses, precedence
- [x] 1.3 An unknown name is an error naming it, never a silent `false`
- [x] 1.4 `terms()` for specificity
- [x] 1.5 Tests: precedence, parentheses, unknown name, specificity counting

## 2. The file
- [x] 2.1 `Binding` with §8.2's fields; `BindingFile.bindings` carries them
- [x] 2.2 Parse both action forms, `[[binding]]` tables and `"none"`
- [x] 2.3 `<leader>` expansion in any step; space-separated sequences
- [x] 2.4 Mouse triggers, per 16 §2.3's spelling
- [x] 2.5 One bad line does not stop the file; problems carry codes
- [x] 2.6 Tests: every form in §8.2's example file, and a broken one beside it

## 3. Refusals that speak
- [x] 3.1 `OMT-C408` unmet gate, `OMT-C410` reserved without `force`,
      `OMT-C421` `modes` under a non-modal keymap
- [x] 3.2 `keys.check_file` checks the fields it documents
- [x] 3.3 Tests: each code fires, and fires only when it should

## 4. Resolution
- [x] 4.1 Compile to ordered bindings with source and specificity
- [x] 4.2 16 §2.3's five rules, first match wins
- [x] 4.3 Pending chord state for multi-step triggers
- [x] 4.4 Tests: each rule, and the passthrough invariant

## 5. Loading it
- [x] 5.1 `omt-config` finds the file per layer, beside `config.toml`
- [x] 5.2 A malformed file refuses rather than silently starting different
- [x] 5.3 Tests: user layer, project layer, precedence between them

## 6. Using it
- [x] 6.1 The leader and its actions come from the resolved map
- [x] 6.2 The keymap flavour comes from the file, not from a config key
      that does not exist
- [x] 6.3 `keys.list` / `keys.cheatsheet` report the resolved map with source
- [x] 6.4 A screen test: a binding file changes what a key does

## 7. The documents
- [x] 7.1 `docs/guide/keybindings.md` and `docs/status.md` match the code
- [x] 7.2 `docs/reference/settings-and-their-readers.md`: the file has a reader

## 8. Gates
- [x] 8.1 fmt, clippy, the five gates in one command
- [x] 8.2 workspace and screen suites; schema snapshot if the wire changed
- [x] 8.3 validate and archive

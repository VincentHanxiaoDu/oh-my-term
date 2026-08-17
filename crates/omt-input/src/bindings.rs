//! The keybinding file: its grammar, and what it means.
//!
//! `docs/architecture/10-configuration.md` §8.2 owns the shape and
//! `docs/architecture/16-input-and-keymap.md` §2.3 owns what it means. Both
//! existed for a long time before anything opened the file: `keybindings.toml`
//! was specified, documented to users and validated by a capability, and no
//! code read it. A rebind did nothing and said nothing.
//!
//! Two rules run through this module, and both come from that history:
//!
//! - **A binding that is not installed says so.** Every refusal carries a code
//!   and a sentence. A binding dropped in silence is indistinguishable from one
//!   nobody wrote.
//! - **One bad line does not lose the file.** A file with forty bindings and
//!   one typo installs thirty-nine and reports the one, because refusing the
//!   whole file over a typo means somebody's keyboard changes under them for a
//!   reason they cannot see.

use serde_json::Value;

use crate::context::{Predicate, PredicateError};
use crate::key::{Chord, ChordParseError};
use crate::mouse::{Button, Kind, Mods, Wheel};

/// Where a binding came from.
///
/// 16 §2.3 rule 4 breaks specificity ties by layer, and `keys.list` reports
/// provenance — "which file did this come from" is the question somebody has
/// when a key does something they did not ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// omt's own defaults.
    Builtin,
    /// The user's file.
    User,
    /// The project's file.
    Project,
    /// Set for this instance.
    Instance,
    /// Set for this session.
    Session,
    /// Set at runtime, above every file.
    Runtime,
}

/// What a binding does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingAction {
    /// Invoke a capability, with arguments.
    Capability {
        /// Its name.
        name: String,
        /// Its arguments, as written. `Value::Null` when there were none.
        args: Value,
    },
    /// `"none"`: explicitly unbound.
    ///
    /// Distinct from having no binding at all, so a user can remove a default
    /// without knowing which layer put it there. Resolution stops here and
    /// falls through to passthrough.
    Unbind,
}

/// One step of a trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A key chord.
    Key(Chord),
    /// A mouse event with modifiers. Never part of a sequence: a two-step
    /// mouse chord is not a gesture any terminal can deliver.
    Mouse(MouseTrigger),
}

/// A mouse event as a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseTrigger {
    /// What happened. `Press` when the spelling omits it.
    pub kind: Kind,
    /// Which button, or which way the wheel turned.
    pub button: Button,
    /// What was held.
    pub mods: Mods,
}

/// One binding, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The trigger, exactly as written — before `<leader>` is expanded, so a
    /// problem can quote what the user typed.
    pub trigger: String,
    /// The `when` predicate, if there is one.
    pub when: Option<String>,
    /// What it does.
    pub action: BindingAction,
    /// Whether it may take a chord on the refusal list.
    pub force: bool,
    /// Which platforms it applies on. Empty means all of them.
    pub platform: Vec<String>,
    /// A terminal capability it needs.
    pub requires: Option<String>,
    /// Modal keymap modes it is restricted to.
    pub modes: Vec<String>,
    /// Whether it is exempt from the key-repeat guard.
    pub repeatable: bool,
}

impl Binding {
    /// The plain form: a trigger and a capability name.
    #[must_use]
    pub fn simple(trigger: &str, capability: &str) -> Self {
        Self {
            trigger: trigger.to_owned(),
            when: None,
            action: if capability == "none" {
                BindingAction::Unbind
            } else {
                BindingAction::Capability {
                    name: capability.to_owned(),
                    args: Value::Null,
                }
            },
            force: false,
            platform: Vec::new(),
            requires: None,
            modes: Vec::new(),
            repeatable: false,
        }
    }
}

/// A keybinding file.
///
/// Flat because it is greppable, diffable and machine-editable; context lives
/// on the binding rather than in nesting. Nesting reads better in an example
/// and worse in a real file — a binding somebody is hunting for is found by
/// searching for the chord, and a nested file hides which context they landed
/// in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingFile {
    /// The leader, which relocates the whole namespace with one edit.
    pub leader: String,
    /// Which flavour: `default`, `vim`, `emacs`, or a name.
    pub keymap: String,
    /// Every binding, in file order.
    pub bindings: Vec<Binding>,
}

impl Default for BindingFile {
    fn default() -> Self {
        Self {
            leader: DEFAULT_LEADER.to_owned(),
            keymap: "default".to_owned(),
            bindings: Vec::new(),
        }
    }
}

/// The leader when the file does not say.
///
/// 16 §3.2: `Ctrl-B`, which is the one the largest group of incoming users
/// already has in their fingers.
pub const DEFAULT_LEADER: &str = "ctrl-b";

/// The diagnostic codes this module can raise.
///
/// Beside the problems rather than in a document, so a code somebody meets in
/// their terminal can be found by grepping the source they were told about.
pub mod codes {
    /// A trigger that is not a chord or a mouse spelling.
    pub const BAD_TRIGGER: &str = "OMT-C401";
    /// An action that names no capability, or an unreadable `when`.
    pub const BAD_ACTION: &str = "OMT-C402";
    /// A gate — `platform` or `requires` — that is not met here.
    pub const GATE_UNMET: &str = "OMT-C408";
    /// A reserved chord bound globally without `force`.
    pub const RESERVED: &str = "OMT-C410";
    /// `modes` declared under a non-modal keymap.
    pub const MODES_WITHOUT_MODAL: &str = "OMT-C421";
}

/// Something wrong with a binding, with the code to look it up by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingProblem {
    /// The stable, greppable code.
    pub code: &'static str,
    /// The trigger it concerns.
    pub trigger: String,
    /// What is wrong.
    pub message: String,
    /// What to do.
    pub help: String,
}

/// Read a keybinding file.
///
/// Returns what could be read and everything that could not. Never an
/// all-or-nothing `Result`: a file is a user's whole keyboard, and one
/// mistyped line must not take the other thirty-nine with it. The one thing
/// that *is* fatal — TOML that does not parse at all — is reported by the
/// caller that read the bytes, because at that point there is no file to speak
/// of.
#[must_use]
pub fn parse(document: &toml_edit::DocumentMut) -> (BindingFile, Vec<BindingProblem>) {
    let mut problems = Vec::new();
    let mut file = BindingFile::default();

    if let Some(leader) = document.get("leader").and_then(|i| i.as_str()) {
        if Chord::parse(leader).is_ok() {
            file.leader = leader.to_owned();
        } else {
            problems.push(BindingProblem {
                code: codes::BAD_TRIGGER,
                trigger: "leader".to_owned(),
                message: format!("`{leader}` is not a chord"),
                help: "Write it like `ctrl-b`.".to_owned(),
            });
        }
    }
    if let Some(keymap) = document.get("keymap").and_then(|i| i.as_str()) {
        file.keymap = keymap.to_owned();
    }

    for (key, item) in document.as_table().iter() {
        match key {
            "leader" | "keymap" => {}
            "binding" => collect_binding_tables(item, &mut file, &mut problems),
            trigger => match binding_from_item(trigger, item) {
                Ok(binding) => file.bindings.push(binding),
                Err(problem) => problems.push(problem),
            },
        }
    }

    (file, problems)
}

/// Read a keybinding file from its text.
///
/// The entry point for anything that has the file rather than a parsed
/// document, which is everything outside the crate that owns TOML. Keeping the
/// grammar behind one function is what stops a second reader of the same file
/// growing a second opinion about it.
///
/// # Errors
/// The one fatal case: text that is not TOML at all. There is no file to
/// report problems in.
pub fn parse_text(text: &str) -> Result<(BindingFile, Vec<BindingProblem>), String> {
    let document: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    Ok(parse(&document))
}

/// The `[[binding]]` array-of-tables form.
fn collect_binding_tables(
    item: &toml_edit::Item,
    file: &mut BindingFile,
    problems: &mut Vec<BindingProblem>,
) {
    let Some(array) = item.as_array_of_tables() else {
        problems.push(BindingProblem {
            code: codes::BAD_ACTION,
            trigger: "binding".to_owned(),
            message: "`binding` is the array-of-tables form".to_owned(),
            help: "Write it as `[[binding]]`, with `trigger` and `capability` inside.".to_owned(),
        });
        return;
    };
    for table in array {
        let trigger = table
            .get("trigger")
            .and_then(|i| i.as_str())
            .unwrap_or_default()
            .to_owned();
        if trigger.is_empty() {
            problems.push(BindingProblem {
                code: codes::BAD_TRIGGER,
                trigger: "[[binding]]".to_owned(),
                message: "a binding table has no `trigger`".to_owned(),
                help: "Give it `trigger = \"ctrl-shift-p\"`.".to_owned(),
            });
            continue;
        }
        let capability = table
            .get("capability")
            .and_then(|i| i.as_str())
            .unwrap_or_default();
        if capability.is_empty() {
            problems.push(BindingProblem {
                code: codes::BAD_ACTION,
                trigger: trigger.clone(),
                message: "the binding names no capability".to_owned(),
                help: "Give it `capability = \"pane.split\"`, or `\"none\"` to unbind.".to_owned(),
            });
            continue;
        }
        let mut binding = Binding::simple(&trigger, capability);
        if let BindingAction::Capability { args, .. } = &mut binding.action {
            *args = table.get("args").map_or(Value::Null, item_to_json);
        }
        binding.when = table
            .get("when")
            .and_then(|i| i.as_str())
            .map(str::to_owned);
        binding.force = table
            .get("force")
            .and_then(|i| i.as_bool())
            .unwrap_or(false);
        binding.repeatable = table
            .get("repeatable")
            .and_then(|i| i.as_bool())
            .unwrap_or(false);
        binding.requires = table
            .get("requires")
            .and_then(|i| i.as_str())
            .map(str::to_owned);
        binding.platform = string_array(table.get("platform"));
        binding.modes = string_array(table.get("modes"));
        file.bindings.push(binding);
    }
}

/// A top-level `trigger = action` entry, in either action form.
fn binding_from_item(trigger: &str, item: &toml_edit::Item) -> Result<Binding, BindingProblem> {
    if let Some(name) = item.as_str() {
        return Ok(Binding::simple(trigger, name));
    }
    // The inline-table form: `{ capability = "...", args = { ... } }`.
    let table = item
        .as_inline_table()
        .cloned()
        .map(toml_edit::InlineTable::into_table)
        .or_else(|| item.as_table().cloned())
        .ok_or_else(|| BindingProblem {
            code: codes::BAD_ACTION,
            trigger: trigger.to_owned(),
            message: "an action is a capability name or a table with `capability`".to_owned(),
            help: "Write `\"<leader> c\" = \"session.create\"`, or a table with `capability` and `args`."
                .to_owned(),
        })?;
    let capability = table
        .get("capability")
        .and_then(|i| i.as_str())
        .ok_or_else(|| BindingProblem {
            code: codes::BAD_ACTION,
            trigger: trigger.to_owned(),
            message: "the table form needs `capability`".to_owned(),
            help: "Add `capability = \"pane.split\"`.".to_owned(),
        })?;
    let mut binding = Binding::simple(trigger, capability);
    if let BindingAction::Capability { args, .. } = &mut binding.action {
        *args = table.get("args").map_or(Value::Null, item_to_json);
    }
    binding.when = table
        .get("when")
        .and_then(|i| i.as_str())
        .map(str::to_owned);
    Ok(binding)
}

fn string_array(item: Option<&toml_edit::Item>) -> Vec<String> {
    item.and_then(|i| i.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn item_to_json(item: &toml_edit::Item) -> Value {
    match item {
        toml_edit::Item::Value(v) => value_to_json(v),
        toml_edit::Item::Table(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.to_owned(), item_to_json(v)))
                .collect(),
        ),
        toml_edit::Item::ArrayOfTables(a) => Value::Array(
            a.iter()
                .map(|t| {
                    Value::Object(
                        t.iter()
                            .map(|(k, v)| (k.to_owned(), item_to_json(v)))
                            .collect(),
                    )
                })
                .collect(),
        ),
        toml_edit::Item::None => Value::Null,
    }
}

fn value_to_json(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(s) => Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => Value::from(*i.value()),
        toml_edit::Value::Float(f) => Value::from(*f.value()),
        toml_edit::Value::Boolean(b) => Value::Bool(*b.value()),
        toml_edit::Value::Datetime(d) => Value::String(d.value().to_string()),
        toml_edit::Value::Array(a) => Value::Array(a.iter().map(value_to_json).collect()),
        toml_edit::Value::InlineTable(t) => Value::Object(
            t.iter()
                .map(|(k, v)| (k.to_owned(), value_to_json(v)))
                .collect(),
        ),
    }
}

/// Expand `<leader>` and read a trigger into its steps.
///
/// `<leader>` is legal in any position and expands to whatever the file's
/// `leader` names: the point of it is that relocating the prefix is one edit
/// rather than forty. Writing the literal `ctrl-b` still means literally
/// `ctrl-b`.
///
/// # Errors
/// Names the step that is not a chord or a mouse spelling.
pub fn steps(trigger: &str, leader: &str) -> Result<Vec<Step>, TriggerError> {
    let mut out = Vec::new();
    for word in trigger.split_whitespace() {
        let word = if word.eq_ignore_ascii_case("<leader>") {
            leader
        } else {
            word
        };
        if let Some(mouse) = mouse_trigger(word) {
            if !out.is_empty() || trigger.split_whitespace().count() > 1 {
                return Err(TriggerError::MouseInSequence {
                    trigger: trigger.to_owned(),
                });
            }
            out.push(Step::Mouse(mouse));
            continue;
        }
        match Chord::parse(word) {
            Ok(chord) => out.push(Step::Key(chord)),
            Err(err) => {
                return Err(TriggerError::NotAChord {
                    step: word.to_owned(),
                    detail: err,
                });
            }
        }
    }
    if out.is_empty() {
        return Err(TriggerError::Empty);
    }
    Ok(out)
}

/// A trigger that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TriggerError {
    /// Nothing to bind.
    #[error("the trigger is empty")]
    Empty,
    /// A step that is not a chord.
    #[error("`{step}` is not a chord: {detail}")]
    NotAChord {
        /// The step as written.
        step: String,
        /// What the chord parser said.
        detail: ChordParseError,
    },
    /// A mouse step inside a sequence.
    #[error("`{trigger}` puts a mouse event in a sequence")]
    MouseInSequence {
        /// The trigger as written.
        trigger: String,
    },
}

/// Read a mouse trigger, or `None` when the word is not one.
///
/// `mod-…-mouse<N>` with an optional `-up` or `-drag`, or `wheel-<direction>`.
/// A bare `shift-mouse1` means press: activation that fires on release is the
/// binding's business, not the grammar's.
#[must_use]
pub fn mouse_trigger(word: &str) -> Option<MouseTrigger> {
    let lower = word.to_lowercase();
    let mut mods = Mods::default();
    let mut kind = Kind::Press;
    let mut button = None;

    let mut parts = lower.split('-').peekable();
    while let Some(part) = parts.next() {
        match part {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" | "option" | "opt" | "meta" => mods.alt = true,
            "shift" => mods.shift = true,
            "wheel" => {
                button = Some(Button::Wheel(match parts.next()? {
                    "up" => Wheel::Up,
                    "down" => Wheel::Down,
                    "left" => Wheel::Left,
                    "right" => Wheel::Right,
                    _ => return None,
                }));
            }
            "up" if button.is_some() => kind = Kind::Release,
            "drag" if button.is_some() => kind = Kind::Drag,
            other => {
                let number = other.strip_prefix("mouse")?;
                button = Some(match number {
                    "1" => Button::Left,
                    "2" => Button::Middle,
                    "3" => Button::Right,
                    // A number omt has no name for is still a button, and
                    // reporting nothing for it would make a five-button mouse
                    // silently unbindable.
                    _ if number.parse::<u8>().is_ok() => Button::None,
                    _ => return None,
                });
            }
        }
    }
    Some(MouseTrigger {
        kind,
        button: button?,
        mods,
    })
}

/// Read a binding's `when`, or `Always` when it has none.
///
/// # Errors
/// Passes on what the predicate parser said, because an unknown context is a
/// binding that would never fire and the user needs the word named.
pub fn predicate_of(binding: &Binding) -> Result<Predicate, PredicateError> {
    binding
        .when
        .as_deref()
        .map_or(Ok(Predicate::Always), Predicate::parse)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::{Binding, BindingAction, Step, TriggerError, codes, mouse_trigger, parse, steps};
    use crate::mouse::{Button, Kind, Wheel};

    fn read(text: &str) -> (super::BindingFile, Vec<super::BindingProblem>) {
        let document: toml_edit::DocumentMut = text.parse().expect("the fixture is valid TOML");
        parse(&document)
    }

    fn binding<'a>(file: &'a super::BindingFile, trigger: &str) -> &'a Binding {
        file.bindings
            .iter()
            .find(|b| b.trigger == trigger)
            .unwrap_or_else(|| panic!("`{trigger}` is not in the file"))
    }

    #[test]
    fn the_documented_example_file_reads() {
        // 10 §8.2's own example, which nothing had ever parsed.
        let (file, problems) = read(
            r#"
leader = "ctrl-b"
keymap = "default"

"<leader> c" = "session.create"
"<leader> x" = { capability = "session.close", args = { confirm = true } }
"<leader> |" = { capability = "pane.split", args = { direction = "vertical" } }
"<leader> z" = "pane.zoom"
"<leader> d" = "none"
"ctrl-shift-p" = "ui.open_command_palette"

[[binding]]
trigger = "enter"
when = "card_focused"
capability = "interaction.resolve"

[[binding]]
trigger = "ctrl-c"
when = "terminal_focused && !copy_mode"
force = true
capability = "session.send_keys"
args = { keys = "" }

[[binding]]
trigger = "ctrl-shift-v"
requires = "kitty_keyboard"
platform = ["macos", "linux"]
capability = "media.image.paste"

[[binding]]
trigger = "d d"
modes = ["normal"]
capability = "explorer.delete"

[[binding]]
trigger = "<leader> ctrl-h"
repeatable = true
capability = "pane.resize"
args = { direction = "left" }
"#,
        );
        assert_eq!(problems, Vec::new(), "the example file has no problems");
        assert_eq!(file.leader, "ctrl-b");
        assert_eq!(file.bindings.len(), 11);

        // Every field the document specifies survives the read.
        assert_eq!(binding(&file, "<leader> d").action, BindingAction::Unbind);
        match &binding(&file, "<leader> x").action {
            BindingAction::Capability { name, args } => {
                assert_eq!(name, "session.close");
                assert_eq!(args["confirm"], serde_json::json!(true));
            }
            other => panic!("wrong action: {other:?}"),
        }
        assert_eq!(
            binding(&file, "enter").when.as_deref(),
            Some("card_focused")
        );
        assert!(binding(&file, "ctrl-c").force);
        assert!(binding(&file, "<leader> ctrl-h").repeatable);
        assert_eq!(
            binding(&file, "ctrl-shift-v").requires.as_deref(),
            Some("kitty_keyboard")
        );
        assert_eq!(binding(&file, "ctrl-shift-v").platform, ["macos", "linux"]);
        assert_eq!(binding(&file, "d d").modes, ["normal"]);
    }

    #[test]
    fn one_bad_line_does_not_lose_the_others() {
        // Refusing the whole file over one typo changes somebody's whole
        // keyboard for a reason they cannot see.
        let (file, problems) = read(
            r#"
leader = "not a chord"
"<leader> c" = "session.create"
"ctrl-p" = 7
"#,
        );
        assert_eq!(
            file.leader,
            super::DEFAULT_LEADER,
            "the bad leader is not taken"
        );
        assert_eq!(file.bindings.len(), 1);
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().any(|p| p.code == codes::BAD_TRIGGER));
        assert!(problems.iter().any(|p| p.code == codes::BAD_ACTION));
    }

    #[test]
    fn leader_expands_wherever_it_appears() {
        let expanded = steps("<leader> ctrl-h", "ctrl-a").expect("the trigger reads");
        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[0], Step::Key(crate::Chord::ctrl('a')));
        // And the literal spelling still means itself.
        assert_eq!(
            steps("ctrl-b c", "ctrl-a").expect("reads")[0],
            Step::Key(crate::Chord::ctrl('b'))
        );
    }

    #[test]
    fn a_mouse_trigger_is_a_step_of_its_own() {
        let shift_click = mouse_trigger("shift-mouse1").expect("a mouse trigger");
        assert_eq!(shift_click.button, Button::Left);
        assert_eq!(shift_click.kind, Kind::Press);
        assert!(shift_click.mods.shift);
        assert_eq!(
            mouse_trigger("wheel-up").expect("a wheel trigger").button,
            Button::Wheel(Wheel::Up)
        );
        assert_eq!(
            mouse_trigger("alt-mouse3-drag").expect("a drag").kind,
            Kind::Drag
        );
        assert!(mouse_trigger("ctrl-p").is_none(), "a key is not a mouse");
        // A two-step mouse chord is not a gesture any terminal can deliver.
        assert!(matches!(
            steps("<leader> mouse1", "ctrl-b"),
            Err(TriggerError::MouseInSequence { .. })
        ));
    }

    #[test]
    fn a_step_that_is_not_a_chord_names_itself() {
        match steps("<leader> nonsensekey", "ctrl-b") {
            Err(TriggerError::NotAChord { step, .. }) => assert_eq!(step, "nonsensekey"),
            other => panic!("wrong outcome: {other:?}"),
        }
    }

    #[test]
    fn a_binding_array_that_is_not_an_array_says_so() {
        let (_, problems) = read("binding = \"pane.split\"\n");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, codes::BAD_ACTION);
    }
}

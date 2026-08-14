//! Keys.
//!
//! Split out of one 6454-line module, which P1's own file-length rule
//! calls a design smell and which nothing measured until the check that
//! enforces it was written.

use super::*;

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// Input to `keys.cheatsheet`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct KeysIn {}

/// One line of the cheatsheet.
#[derive(Serialize, schemars::JsonSchema)]
pub struct KeyBinding {
    /// The chord, canonically spelled.
    pub chord: String,
    /// Which mode it applies in.
    pub mode: String,
    /// What it does, or why it is reserved.
    pub action: String,
    /// Whether the program underneath sees this key.
    pub reaches_program: bool,
}

/// What `keys.cheatsheet` reports.
#[derive(Serialize, schemars::JsonSchema)]
pub struct KeysOut {
    /// Every binding, and every reserved key.
    pub bindings: Vec<KeyBinding>,
}

capability! {
    /// Every key, generated from the live keymap.
    pub struct KeysCheatsheet;
    input  = KeysIn,
    output = KeysOut,
    decl = Decl {
        name: "keys.cheatsheet",
        group: "keys",
        verb: "cheatsheet",
        title: "Keyboard reference",
        aliases: &["keys"],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Query,
        role: Role::Viewer,
        effects: Effects::empty(),
        intent: None,
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Every binding and every reserved key, generated from the keymap \
              in force so it cannot go stale.",
    },
}

pub(crate) struct KeysHandler;

impl CapabilityHandler<KeysCheatsheet> for KeysHandler {
    fn call(&self, _ctx: &CallContext, _input: KeysIn) -> Result<KeysOut, CapabilityError> {
        Ok(KeysOut {
            bindings: omt_input::cheatsheet(&omt_input::defaults())
                .into_iter()
                .map(|e| KeyBinding {
                    chord: e.chord,
                    mode: format!("{:?}", e.mode).to_lowercase(),
                    action: e.action,
                    reaches_program: e.reaches_program,
                })
                .collect(),
        })
    }
}

pub(crate) fn session_id(text: &str) -> Result<SessionId, CapabilityError> {
    SessionId::from_wire(text)
        .ok_or_else(|| CapabilityError::invalid_input(format!("`{text}` is not a session id")))
}

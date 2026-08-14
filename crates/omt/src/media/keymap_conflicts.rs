//! Keymap conflicts, Directional focus.
//!
//! Split out of one 3478-line module, over P1's 2000-line limit.

use super::*;

// ---------------------------------------------------------------------------
// Keymap conflicts — 16 §5
// ---------------------------------------------------------------------------

/// Input to `keys.conflicts`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConflictsIn {
    /// The chord omt would take.
    pub chord: String,
}

/// One thing a binding would break.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ConflictOut {
    /// Which program.
    pub program: String,
    /// What the key does there, in that program's own words.
    pub does: String,
    /// How much losing it matters.
    pub severity: String,
    /// What that keymap was checked against, so a stale warning is visible.
    pub verified_against: String,
}

/// What `keys.conflicts` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ConflictsOut {
    /// The canonical form of the chord that was asked about.
    pub chord: String,
    /// Everything it would break, worst first.
    pub conflicts: Vec<ConflictOut>,
    /// Whether omt refuses to take it by default.
    pub refused: bool,
}

omt_catalog::capability! {
    /// What binding this chord would break in the program underneath.
    pub struct KeysConflicts;
    input  = ConflictsIn,
    output = ConflictsOut,
    decl = omt_catalog::Decl {
        name: "keys.conflicts",
        group: "keys",
        verb: "conflicts",
        title: "What a binding would break",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "`Conflicts with something` is not actionable — every chord collides somewhere. This says which program, what the key does there, and how badly it would be missed.",
    },
}

/// Serves the keymap conflict registry.
pub struct KeysHandler;

impl CapabilityHandler<KeysConflicts> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ConflictsIn,
    ) -> Result<ConflictsOut, CapabilityError> {
        let chord = omt_input::key::Chord::parse(&input.chord).map_err(|e| {
            CapabilityError::invalid_input(format!("`{}` is not a chord: {e}", input.chord))
        })?;
        let registry = omt_input::conflicts::builtin();
        Ok(ConflictsOut {
            refused: registry.should_refuse(&chord),
            chord: chord.canonical(),
            conflicts: registry
                .conflicts(&chord)
                .into_iter()
                .map(|c| ConflictOut {
                    program: c.program,
                    does: c.does,
                    severity: format!("{:?}", c.severity).to_lowercase(),
                    verified_against: c.verified_against,
                })
                .collect(),
        })
    }
}

/// Input to `keys.audit`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct AuditIn {}

/// What `keys.audit` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AuditOut {
    /// Everything omt's own defaults would take, worst first.
    pub conflicts: Vec<ConflictOut>,
    /// How many programs the registry knows about.
    ///
    /// Reported so an empty result can be told apart from a registry that
    /// knows nothing — "no conflicts" from a check that examined nothing looks
    /// exactly like "no conflicts" from one that examined everything.
    pub programs_known: usize,
}

omt_catalog::capability! {
    /// What omt's own default bindings would take from programs underneath.
    pub struct KeysAudit;
    input  = AuditIn,
    output = AuditOut,
    decl = omt_catalog::Decl {
        name: "keys.audit",
        group: "keys",
        verb: "audit",
        title: "Audit omt's own bindings",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A registry nobody runs the defaults against documents a problem instead of preventing one.",
    },
}

impl CapabilityHandler<KeysAudit> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: AuditIn,
    ) -> Result<AuditOut, CapabilityError> {
        let registry = omt_input::conflicts::builtin();
        Ok(AuditOut {
            programs_known: registry.len(),
            conflicts: omt_input::conflicts::audit(&omt_input::keymap::defaults(), &registry)
                .into_iter()
                .map(|c| ConflictOut {
                    program: c.program,
                    does: c.does,
                    severity: format!("{:?}", c.severity).to_lowercase(),
                    verified_against: c.verified_against,
                })
                .collect(),
        })
    }
}

/// Input to `keys.flavour`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct FlavourIn {
    /// `plain`, `vim` or `emacs`.
    pub flavour: String,
}

/// One binding a flavour adds.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FlavourBinding {
    /// Which of omt's own surfaces.
    pub mode: String,
    /// The chord.
    pub chord: String,
    /// What it invokes.
    pub capability: String,
}

/// What `keys.flavour` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FlavourOut {
    /// Everything it binds.
    pub bindings: Vec<FlavourBinding>,
    /// Which modes a modal keymap is allowed to touch at all.
    ///
    /// Reported because it is the rule that makes the feature safe, and a rule
    /// nobody can read is one nobody can check.
    pub governs: Vec<String>,
}

omt_catalog::capability! {
    /// What a modal keymap flavour binds, and where it is allowed to.
    pub struct KeysFlavour;
    input  = FlavourIn,
    output = FlavourOut,
    decl = omt_catalog::Decl {
        name: "keys.flavour",
        group: "keys",
        verb: "flavour",
        title: "vim or emacs bindings",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "A modal keymap governs omt's own surfaces only. It never governs a pane whose keys are passing through, which is what stops omt and an inner vim from contending over hjkl.",
    },
}

impl CapabilityHandler<KeysFlavour> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: FlavourIn,
    ) -> Result<FlavourOut, CapabilityError> {
        let flavour = match input.flavour.to_lowercase().as_str() {
            "vim" => omt_input::modal::Flavour::Vim,
            "emacs" => omt_input::modal::Flavour::Emacs,
            "plain" => omt_input::modal::Flavour::Plain,
            other => {
                return Err(CapabilityError::invalid_input(format!(
                    "no keymap flavour `{other}`; omt has plain, vim and emacs"
                )));
            }
        };
        Ok(FlavourOut {
            governs: omt_input::modal::OMT_OWNED
                .iter()
                .map(|m| format!("{m:?}").to_lowercase())
                .collect(),
            bindings: omt_input::modal::bindings(flavour)
                .into_iter()
                .map(|(mode, chord, capability)| FlavourBinding {
                    mode: format!("{mode:?}").to_lowercase(),
                    chord: chord.to_owned(),
                    capability: capability.to_owned(),
                })
                .collect(),
        })
    }
}

/// Input to `keys.list`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct KeysListIn {}

/// One binding.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BindingOut {
    /// Which surface it applies on.
    pub mode: String,
    /// The chord.
    pub chord: String,
    /// What it does.
    pub action: String,
}

/// What `keys.list` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct KeysListOut {
    /// Every binding, in a stable order.
    pub bindings: Vec<BindingOut>,
}

omt_catalog::capability! {
    /// Every binding omt currently has.
    pub struct KeysList;
    input  = KeysListIn,
    output = KeysListOut,
    decl = omt_catalog::Decl {
        name: "keys.list",
        group: "keys",
        verb: "list",
        title: "List every binding",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "In a stable order, so two calls can be compared. An unordered list makes a diff between two configurations unreadable.",
    },
}

impl CapabilityHandler<KeysList> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: KeysListIn,
    ) -> Result<KeysListOut, CapabilityError> {
        let keymap = omt_input::keymap::defaults();
        let mut bindings: Vec<BindingOut> = keymap
            .bindings()
            .into_iter()
            .map(|(mode, chord, action)| BindingOut {
                mode: format!("{mode:?}").to_lowercase(),
                chord: chord.canonical(),
                action: match action {
                    omt_input::keymap::Action::Capability { name } => name.clone(),
                    omt_input::keymap::Action::Passthrough => "passthrough".to_owned(),
                    omt_input::keymap::Action::Unbound => "unbound".to_owned(),
                },
            })
            .collect();
        bindings.sort_by(|a, b| a.mode.cmp(&b.mode).then(a.chord.cmp(&b.chord)));
        Ok(KeysListOut { bindings })
    }
}

/// Input to `keys.explain`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainIn {
    /// The chord to explain.
    pub chord: String,
}

/// What `keys.explain` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ExplainOut {
    /// Its canonical form.
    pub chord: String,
    /// What omt does with it, if anything.
    pub action: Option<String>,
    /// On which surface.
    pub mode: Option<String>,
    /// What it would take from the program underneath.
    pub conflicts: Vec<ConflictOut>,
    /// Whether it needs an AltGr on a common non-US layout.
    pub awkward: bool,
    /// The alias installed beside it, where there is one.
    pub alias: Option<String>,
    /// What happens in a pane that is passing through.
    ///
    /// Said plainly, because "not bound" and "bound but the pane owns it" look
    /// identical from a user's chair and have different fixes.
    pub in_a_pane: String,
}

omt_catalog::capability! {
    /// Everything omt knows about one chord.
    pub struct KeysExplain;
    input  = ExplainIn,
    output = ExplainOut,
    decl = omt_catalog::Decl {
        name: "keys.explain",
        group: "keys",
        verb: "explain",
        title: "Explain a key",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "`Not bound` and `bound, but the pane underneath owns it` look identical from the user's chair and have different fixes, so this says which.",
    },
}

impl CapabilityHandler<KeysExplain> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: ExplainIn,
    ) -> Result<ExplainOut, CapabilityError> {
        let chord = omt_input::key::Chord::parse(&input.chord).map_err(|e| {
            CapabilityError::invalid_input(format!("`{}` is not a chord: {e}", input.chord))
        })?;
        let keymap = omt_input::keymap::defaults();
        let found = keymap
            .bindings()
            .into_iter()
            .find(|(_, c, _)| c.canonical() == chord.canonical())
            .map(|(mode, _, action)| {
                (
                    format!("{mode:?}").to_lowercase(),
                    match action {
                        omt_input::keymap::Action::Capability { name } => name.clone(),
                        omt_input::keymap::Action::Passthrough => "passthrough".to_owned(),
                        omt_input::keymap::Action::Unbound => "unbound".to_owned(),
                    },
                )
            });
        let global = found.as_ref().is_some_and(|(mode, _)| mode == "global");
        Ok(ExplainOut {
            in_a_pane: if global {
                "omt handles this before the pane sees it.".to_owned()
            } else {
                "The program in the pane receives this untouched.".to_owned()
            },
            mode: found.as_ref().map(|(mode, _)| mode.clone()),
            action: found.map(|(_, action)| action),
            conflicts: omt_input::conflicts::builtin()
                .conflicts(&chord)
                .into_iter()
                .map(|c| ConflictOut {
                    program: c.program,
                    does: c.does,
                    severity: format!("{:?}", c.severity).to_lowercase(),
                    verified_against: c.verified_against,
                })
                .collect(),
            awkward: omt_input::layout::is_awkward(&chord),
            alias: omt_input::layout::alias_for(&chord).map(str::to_owned),
            chord: chord.canonical(),
        })
    }
}

/// Input to `keys.registry`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegistryIn {}

/// What `keys.registry` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RegistryOut {
    /// How many programs omt knows the keymap of.
    pub programs: usize,
    /// Which keymap flavours exist.
    pub flavours: Vec<String>,
}

omt_catalog::capability! {
    /// What omt knows about other programs' keys.
    pub struct KeysRegistry;
    input  = RegistryIn,
    output = RegistryOut,
    decl = omt_catalog::Decl {
        name: "keys.registry",
        group: "keys",
        verb: "registry",
        title: "Known inner keymaps",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The registry is data, so a new agent CLI is a file rather than a release. This says how much of it is loaded.",
    },
}

impl CapabilityHandler<KeysRegistry> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        _input: RegistryIn,
    ) -> Result<RegistryOut, CapabilityError> {
        Ok(RegistryOut {
            programs: omt_input::conflicts::builtin().len(),
            flavours: vec!["plain".to_owned(), "vim".to_owned(), "emacs".to_owned()],
        })
    }
}

// ---------------------------------------------------------------------------
// Directional focus — 17 §6
// ---------------------------------------------------------------------------

/// One placed pane, as the caller describes it.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct PlacedIn {
    /// Which pane.
    pub id: String,
    /// Column of the left edge.
    pub left: u16,
    /// Row of the top edge.
    pub top: u16,
    /// One past the right edge.
    pub right: u16,
    /// One past the bottom edge.
    pub bottom: u16,
    /// Whether it is currently drawn.
    #[serde(default)]
    pub hidden: bool,
    /// Its stable pre-order index, the total tie-break.
    pub placement_index: u32,
}

/// Input to `pane.navigate`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct NavigateIn {
    /// Every placed pane.
    pub panes: Vec<PlacedIn>,
    /// Where focus is now.
    pub from: String,
    /// `left`, `right`, `up` or `down`.
    pub direction: String,
    /// Whether to jump to the far side when there is nothing that way.
    #[serde(default)]
    pub wrap: bool,
}

/// What `pane.navigate` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct NavigateOut {
    /// Where focus should go, or nothing.
    pub focus: Option<String>,
}

omt_catalog::capability! {
    /// Move focus in a direction, geometrically.
    pub struct PaneNavigate;
    input  = NavigateIn,
    output = NavigateOut,
    decl = omt_catalog::Decl {
        name: "pane.navigate",
        group: "pane",
        verb: "navigate",
        title: "Move focus in a direction",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Viewer,
        effects: omt_catalog::Effects::empty(),
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Geometric, over the placed rectangles rather than tree order — tree order gives wrong answers in asymmetric layouts. Deterministic, because breaking ties by recency makes the same keypress do two different things for reasons the user cannot see.",
    },
}

impl CapabilityHandler<PaneNavigate> for KeysHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: NavigateIn,
    ) -> Result<NavigateOut, CapabilityError> {
        let direction = match input.direction.to_lowercase().as_str() {
            "left" => omt_tui::navigate::Direction::Left,
            "right" => omt_tui::navigate::Direction::Right,
            "up" => omt_tui::navigate::Direction::Up,
            "down" => omt_tui::navigate::Direction::Down,
            other => {
                return Err(CapabilityError::invalid_input(format!(
                    "no direction `{other}`; omt has left, right, up and down"
                )));
            }
        };
        let panes: Vec<omt_tui::navigate::Placed> = input
            .panes
            .into_iter()
            .map(|p| omt_tui::navigate::Placed {
                id: p.id,
                rect: omt_tui::navigate::Rect {
                    left: p.left,
                    top: p.top,
                    right: p.right,
                    bottom: p.bottom,
                },
                hidden: p.hidden,
                placement_index: p.placement_index,
            })
            .collect();
        Ok(NavigateOut {
            focus: omt_tui::navigate::navigate(&panes, &input.from, direction, input.wrap),
        })
    }
}

/// Saved layouts, by name.
pub type Layouts =
    std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, omt_tui::geometry::Saved>>>;

/// Input to `layout.save`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayoutSaveIn {
    /// What to call it.
    pub name: String,
    /// The panes, in the order they should be arranged.
    pub panes: Vec<String>,
    /// `even_vertical`, `even_horizontal`, `main_vertical` or `main_horizontal`.
    pub preset: String,
}

/// What `layout.save` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LayoutSaveOut {
    /// What it was called.
    pub name: String,
    /// How many layouts are saved now.
    pub saved: usize,
}

omt_catalog::capability! {
    /// Save an arrangement under a name.
    pub struct LayoutSave;
    input  = LayoutSaveIn,
    output = LayoutSaveOut,
    decl = omt_catalog::Decl {
        name: "layout.save",
        group: "layout",
        verb: "save",
        title: "Save this layout",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Command,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::WRITES_FS,
        // Cas: saving the same name twice is a replace, so a retry after a
        // dropped acknowledgement lands on the same result.
        intent: Some(omt_catalog::Intent::Cas),
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "The tree and nothing else. Rects are a function of the tree and the screen, so saving them would restore a desktop layout onto a phone as a desktop layout.",
    },
}

/// Serves saved layouts.
pub struct LayoutHandler(pub Layouts);

pub(crate) fn preset_of(name: &str) -> Option<omt_tui::geometry::Preset> {
    match name {
        "even_vertical" => Some(omt_tui::geometry::Preset::EvenVertical),
        "even_horizontal" => Some(omt_tui::geometry::Preset::EvenHorizontal),
        "main_vertical" => Some(omt_tui::geometry::Preset::MainVertical),
        "main_horizontal" => Some(omt_tui::geometry::Preset::MainHorizontal),
        _ => None,
    }
}

impl CapabilityHandler<LayoutSave> for LayoutHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: LayoutSaveIn,
    ) -> Result<LayoutSaveOut, CapabilityError> {
        if input.name.trim().is_empty() {
            return Err(CapabilityError::invalid_input(
                "a layout needs a name to be restored by",
            ));
        }
        let preset = preset_of(&input.preset).ok_or_else(|| {
            CapabilityError::invalid_input(format!(
                "no preset `{}`; omt has even_vertical, even_horizontal, main_vertical and main_horizontal",
                input.preset
            ))
        })?;
        let tree = omt_tui::geometry::preset(preset, &input.panes)
            .ok_or_else(|| CapabilityError::invalid_input("an empty layout is not a layout"))?;
        let mut held = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the layout lock was poisoned"))?;
        held.insert(
            input.name.clone(),
            omt_tui::geometry::Saved {
                name: input.name.clone(),
                tree: omt_tui::geometry::SavedNode::from(&tree),
            },
        );
        Ok(LayoutSaveOut {
            name: input.name,
            saved: held.len(),
        })
    }
}

/// Input to `layout.restore`.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct LayoutRestoreIn {
    /// Which saved layout.
    pub name: String,
    /// Which panes still exist.
    pub alive: Vec<String>,
    /// How wide the screen is.
    pub width: u16,
    /// How tall.
    pub height: u16,
}

/// What `layout.restore` reports.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LayoutRestoreOut {
    /// Where each surviving pane goes.
    pub panes: Vec<PlacementOut>,
    /// Panes the layout named that are no longer running.
    ///
    /// Named rather than counted, because a restore that quietly drops two
    /// panes leaves somebody wondering what they have forgotten.
    pub missing: Vec<String>,
    /// Panes that survived but do not fit this screen.
    pub hidden: Vec<String>,
}

/// One placed pane.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PlacementOut {
    /// Which pane.
    pub pane: String,
    /// Column of the left edge.
    pub left: u16,
    /// Row of the top edge.
    pub top: u16,
    /// How wide.
    pub width: u16,
    /// How tall.
    pub height: u16,
}

omt_catalog::capability! {
    /// Restore a saved layout onto this screen.
    pub struct LayoutRestore;
    input  = LayoutRestoreIn,
    output = LayoutRestoreOut,
    decl = omt_catalog::Decl {
        name: "layout.restore",
        group: "layout",
        verb: "restore",
        title: "Restore a saved layout",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: omt_catalog::Kind::Query,
        role: omt_types::Role::Operator,
        effects: omt_catalog::Effects::READS_FS,
        intent: None,
        parity: omt_catalog::Parity::Full,
        since: "0.1.0",
        doc: "Keeps the panes that still exist and names the rest. Failing outright gives somebody nothing; keeping dead names gives them panes they have to close one by one.",
    },
}

impl CapabilityHandler<LayoutRestore> for LayoutHandler {
    fn call(
        &self,
        _ctx: &omt_catalog::CallContext,
        input: LayoutRestoreIn,
    ) -> Result<LayoutRestoreOut, CapabilityError> {
        let held = self
            .0
            .lock()
            .map_err(|_| CapabilityError::internal("the layout lock was poisoned"))?;
        let saved = held
            .get(&input.name)
            .ok_or_else(|| CapabilityError::not_found(format!("no layout `{}`", input.name)))?;
        let (tree, missing) = omt_tui::geometry::restore(saved, &input.alive).ok_or_else(|| {
            CapabilityError::precondition_failed(format!(
                "none of the panes in `{}` are still running",
                input.name
            ))
        })?;
        let geometry = omt_tui::geometry::compute(
            &tree,
            omt_tui::geometry::Rect {
                left: 0,
                top: 0,
                width: input.width,
                height: input.height,
            },
        );
        Ok(LayoutRestoreOut {
            panes: geometry
                .panes
                .into_iter()
                .map(|p| PlacementOut {
                    pane: p.pane,
                    left: p.outer.left,
                    top: p.outer.top,
                    width: p.outer.width,
                    height: p.outer.height,
                })
                .collect(),
            hidden: geometry.hidden,
            missing,
        })
    }
}

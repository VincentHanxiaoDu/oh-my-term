//! What can be configured, declared once.
//!
//! Values are merged from files; this is the other half — what the keys *are*.
//! Without it a setting is discoverable only by already knowing its name, which
//! makes every setting a thing you have to be told about.
//!
//! Declared rather than derived from whatever happens to be in a file: a key
//! nobody has set still exists, and a settings screen that only lists what
//! somebody already changed is not a settings screen.

/// What a setting holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// True or false.
    Bool,
    /// A whole number.
    Integer,
    /// A decimal.
    Number,
    /// Text.
    Text,
    /// One of a fixed set.
    Enumerated,
    /// Several values, in order.
    ///
    /// Added for font fallbacks, which are a list in every terminal that has
    /// them. Squeezing that into text would make the separator part of the
    /// value, and then every client has to agree on whether it is a comma or a
    /// semicolon.
    List,
}

impl Kind {
    /// Its wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Text => "text",
            Self::Enumerated => "enum",
            Self::List => "list",
        }
    }
}

/// One thing a user can set.
#[derive(Debug, Clone, Copy)]
pub struct Setting {
    /// Its dotted key.
    pub key: &'static str,
    /// What it holds.
    pub kind: Kind,
    /// What it is when nobody has set it, as JSON text.
    ///
    /// A default for everything, because P11's rule is that a feature works
    /// before it is configured — a setting with no default is a setting that
    /// must be set, which is the same failure with extra steps.
    pub default: &'static str,
    /// One line about what it does.
    pub doc: &'static str,
    /// The permitted values, for an enumerated setting.
    pub choices: &'static [&'static str],
    /// Which layers may set it.
    ///
    /// Not decoration. A repository you cloned must not be able to tell your
    /// daemon what address to bind, so `server.bind` is not settable from a
    /// project file — and the merge is where that is enforced, which is why
    /// this lives on the same table the settings screen reads.
    pub scope: crate::Scope,
}

/// The schema, as the merge wants it.
///
/// Derived rather than written twice. `SETTINGS` says a settings screen,
/// `config.schema` and the validator all read one table — and until this
/// existed the validator read an empty slice, so every key was unknown, no
/// scope was enforced and a typo was silently applied.
#[must_use]
pub fn key_specs() -> Vec<crate::KeySpec> {
    SETTINGS
        .iter()
        .map(|s| crate::KeySpec {
            key: s.key,
            scope: s.scope,
        })
        .collect()
}

/// Settings nothing reads yet, and why.
///
/// A setting is a promise. A settings screen that shows a key, validates it,
/// records which file it came from and then changes nothing is worse than no
/// setting at all: it fails in the one way nobody checks for, because it looks
/// exactly like a setting that worked. Fifty-nine of these were found that way
/// — `terminal.scrollback_lines` never reached `ScrollbackLimits`, and
/// `media.images.enabled` could not turn anything off.
///
/// So the list is here rather than in somebody's head, and
/// `cargo xtask settings-readers` fails in *both* directions: a setting with
/// no reader and no entry here fails, and an entry here for a setting that has
/// since been wired up fails as stale. Emptiness has to be declared.
pub const UNWIRED: &[(&str, &str)] = &[
    (
        "terminal.blocks",
        "the block tracker is always on; nothing consults this yet",
    ),
    (
        "terminal.foreground",
        "the host colours come from the environment; nothing consults this yet",
    ),
    (
        "terminal.background",
        "the host colours come from the environment; nothing consults this yet",
    ),
    (
        "agents.detection",
        "detection is unconditional; nothing consults this yet",
    ),
    (
        "appearance.block_style",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.cursor.blink",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.cursor.style",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.density",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.font.fallbacks",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.font.family",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.font.ligatures",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.font.line_height",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.font.size",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.show_agent_badges",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.theme",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.theme_light",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "appearance.theme_mode",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "font.family",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "font.line_height",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "font.size",
        "the web client draws with it; nothing applies it yet",
    ),
    (
        "hints.enabled",
        "the hint line is state-driven; nothing consults this yet",
    ),
    (
        "log.file",
        "logging goes to stderr and the crash note; nothing consults this yet",
    ),
    (
        "log.level",
        "logging goes to stderr and the crash note; nothing consults this yet",
    ),
    (
        "log.max_size",
        "logging goes to stderr and the crash note; nothing consults this yet",
    ),
    (
        "log.redact_secrets",
        "logging goes to stderr and the crash note; nothing consults this yet",
    ),
    (
        "media.blob_dir",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.clipboard.osc52_max_bytes",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.clipboard.osc52_read",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.clipboard.osc52_write",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.images.enabled",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.images.max_rows",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.images.protocol",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.images.thumbnail_max_px",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.osc_bridge.max_bytes",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.quota.max_blob_bytes",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.quota.max_blobs_per_session",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.quota.max_total_bytes",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.quota.ttl",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.quota.ttl_referenced",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "media.reverse_socket.hosts",
        "the media path is unconditional; nothing consults this yet",
    ),
    (
        "server.bind",
        "the address comes from the command line; nothing consults this yet",
    ),
    (
        "shell.auto_install",
        "shell integration installs the same way for everyone; nothing consults this yet",
    ),
    (
        "shell.command_metadata",
        "shell integration installs the same way for everyone; nothing consults this yet",
    ),
    (
        "shell.inject_env",
        "shell integration installs the same way for everyone; nothing consults this yet",
    ),
    (
        "shell.osc133",
        "shell integration installs the same way for everyone; nothing consults this yet",
    ),
    (
        "shell.propagate_over_ssh",
        "shell integration installs the same way for everyone; nothing consults this yet",
    ),
    (
        "shell.shells",
        "shell integration installs the same way for everyone; nothing consults this yet",
    ),
    (
        "terminal.bell",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.blocks.enabled",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.blocks.heuristic_fallback",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.blocks.max_retained",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.bracketed_paste",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.default_cwd",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.default_shell",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.env_passthrough",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.highlight",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.mouse_reporting",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.paste_confirm_lines",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.paste_confirm_on_newline",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.reflow.on_resize",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.scrollback_lines",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
    (
        "terminal.word_separators",
        "the terminal core takes its own defaults; nothing passes this in yet",
    ),
];

/// Every setting omt understands.
///
/// One table. A settings screen, `config.schema` and the validator all read it,
/// so a key cannot exist in one and be missing from another.
pub const SETTINGS: &[Setting] = &[
    Setting {
        key: "appearance.theme_light",
        kind: Kind::Text,
        default: "\"omt-light\"",
        doc: "used when the OS reports light mode.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.theme_mode",
        kind: Kind::Enumerated,
        default: "\"auto\"",
        doc: "Theme mode.",
        choices: &["auto", "dark", "light"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.density",
        kind: Kind::Enumerated,
        default: "\"comfortable\"",
        doc: "affects TUI chrome and web padding.",
        choices: &["compact", "comfortable"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.show_agent_badges",
        kind: Kind::Bool,
        default: "true",
        doc: "agent-state badges on panes/tabs.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.block_style",
        kind: Kind::Enumerated,
        default: "\"gutter\"",
        doc: "how command blocks are delimited.",
        choices: &["bordered", "gutter", "minimal"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.cursor.style",
        kind: Kind::Enumerated,
        default: "\"block\"",
        doc: "The appearance cursor style.",
        choices: &["block", "bar", "underline"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.cursor.blink",
        kind: Kind::Bool,
        default: "false",
        doc: "The appearance cursor blink.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.font.family",
        kind: Kind::Text,
        default: "\"monospace\"",
        doc: "The appearance font family.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.font.size",
        kind: Kind::Number,
        default: "13.0",
        doc: "The appearance font size.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.font.line_height",
        kind: Kind::Number,
        default: "1.2",
        doc: "Line height.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.font.ligatures",
        kind: Kind::Bool,
        default: "false",
        doc: "The appearance font ligatures.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.font.fallbacks",
        kind: Kind::List,
        default: "[]",
        doc: "The appearance font fallbacks.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.default_shell",
        kind: Kind::Text,
        default: "\"\"",
        doc: "The shell new sessions start. Empty means omt uses $SHELL.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.default_cwd",
        kind: Kind::Enumerated,
        default: "\"workspace\"",
        doc: "Default cwd.",
        choices: &["home", "last", "workspace"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.word_separators",
        kind: Kind::Text,
        default: "\" \\t[]{}'\\\"<>,;:\"",
        doc: "Characters that end a word when double-clicking or moving by word.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.bell",
        kind: Kind::Enumerated,
        default: "\"visual\"",
        doc: "The terminal bell.",
        choices: &["off", "visual", "audible", "both"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.mouse_reporting",
        kind: Kind::Bool,
        default: "true",
        doc: "Mouse reporting.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.bracketed_paste",
        kind: Kind::Bool,
        default: "true",
        doc: "Bracketed paste.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.paste_confirm_lines",
        kind: Kind::Integer,
        default: "10",
        doc: "Paste confirm lines.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.paste_confirm_on_newline",
        kind: Kind::Bool,
        default: "true",
        doc: "Paste confirm on newline.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.env_passthrough",
        kind: Kind::List,
        default: "[\"TERM_PROGRAM\",\"COLORTERM\",\"LANG\"]",
        doc: "Env passthrough.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.blocks.enabled",
        kind: Kind::Bool,
        default: "true",
        doc: "The terminal blocks enabled.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.blocks.heuristic_fallback",
        kind: Kind::Bool,
        default: "true",
        doc: "Heuristic fallback.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.blocks.max_retained",
        kind: Kind::Integer,
        default: "2000",
        doc: "Max retained.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.reflow.on_resize",
        kind: Kind::Enumerated,
        default: "\"active_block\"",
        doc: "The terminal reflow on resize.",
        choices: &["active_block", "all", "none"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "shell.auto_install",
        kind: Kind::Bool,
        default: "true",
        doc: "Auto install.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "shell.shells",
        kind: Kind::Text,
        default: "\"\"",
        doc: "The shell shells. Default: auto-detected.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "shell.osc133",
        kind: Kind::Enumerated,
        default: "\"both\"",
        doc: "The shell osc133.",
        choices: &["emit", "consume", "both"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "shell.inject_env",
        kind: Kind::Bool,
        default: "false",
        doc: "Inject env.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "shell.propagate_over_ssh",
        kind: Kind::Bool,
        default: "false",
        doc: "Propagate over ssh.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "shell.command_metadata",
        kind: Kind::Bool,
        default: "false",
        doc: "Command metadata.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.quota.max_blob_bytes",
        kind: Kind::Text,
        default: "\"32MiB\"",
        doc: "Max blob bytes.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.quota.max_total_bytes",
        kind: Kind::Text,
        default: "\"512MiB\"",
        doc: "Max total bytes.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.quota.max_blobs_per_session",
        kind: Kind::Integer,
        default: "200",
        doc: "Max blobs per session.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.quota.ttl",
        kind: Kind::Integer,
        default: "24",
        doc: "The media quota ttl.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.quota.ttl_referenced",
        kind: Kind::Integer,
        default: "7",
        doc: "Ttl referenced.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.blob_dir",
        kind: Kind::Text,
        default: "\"\"",
        doc: "The media blob dir. Default: $XDG_RUNTIME_DIR/omt/<instance>/blobs.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.images.enabled",
        kind: Kind::Bool,
        default: "true",
        doc: "The media images enabled.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.images.protocol",
        kind: Kind::Enumerated,
        default: "\"auto\"",
        doc: "The media images protocol.",
        choices: &["auto", "kitty", "sixel", "iterm2", "none"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.images.max_rows",
        kind: Kind::Integer,
        default: "20",
        doc: "The media images max rows.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.images.thumbnail_max_px",
        kind: Kind::Integer,
        default: "512",
        doc: "Thumbnail max px.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.clipboard.osc52_write",
        kind: Kind::Bool,
        default: "true",
        doc: "Osc52 write.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.clipboard.osc52_max_bytes",
        kind: Kind::Text,
        default: "\"64KiB\"",
        doc: "Osc52 max bytes.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.clipboard.osc52_read",
        kind: Kind::Bool,
        default: "false",
        doc: "Osc52 read.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.osc_bridge.max_bytes",
        kind: Kind::Text,
        default: "\"8MiB\"",
        doc: "The media osc bridge max bytes.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "media.reverse_socket.hosts",
        kind: Kind::List,
        default: "[]",
        doc: "The media reverse socket hosts.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "log.level",
        kind: Kind::Enumerated,
        default: "\"info\"",
        doc: "The log level.",
        choices: &["error", "warn", "info", "debug", "trace"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "log.file",
        kind: Kind::Text,
        default: "\"\"",
        doc: "The log file. Default: $XDG_STATE_HOME/omt/omt.log.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "log.max_size",
        kind: Kind::Text,
        default: "\"32MiB\"",
        doc: "The log max size.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "log.redact_secrets",
        kind: Kind::Bool,
        default: "false",
        doc: "Redact secrets.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "font.family",
        kind: Kind::Text,
        default: "\"monospace\"",
        doc: "The font a client renders the terminal in.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "font.size",
        kind: Kind::Number,
        default: "13.0",
        doc: "Point size.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "font.line_height",
        kind: Kind::Number,
        default: "1.2",
        doc: "Line height as a multiple of the font size.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "appearance.theme",
        kind: Kind::Text,
        default: "\"omt dark\"",
        doc: "The theme, by name. Built in, or one imported into the themes directory.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.scrollback_lines",
        kind: Kind::Integer,
        default: "10000",
        doc: "How many lines of scrollback each session keeps.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.blocks",
        kind: Kind::Bool,
        default: "true",
        doc: "Track command blocks. On for every shell; a shell that emits no marks still gets blocks, with less known about each.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.highlight",
        kind: Kind::Bool,
        default: "true",
        doc: "Colour the command line by token.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "hints.enabled",
        kind: Kind::Bool,
        default: "true",
        doc: "Show the hint line. It stops offering the introduction once the prefix has been used.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "server.bind",
        kind: Kind::Text,
        default: "\"127.0.0.1:7717\"",
        doc: "Where the web surface listens. Loopback by default, deliberately.",
        choices: &[],
        // A repository you cloned must not be able to move your daemon off
        // loopback. This is the key where that matters most, and the scope is
        // what stops it.
        scope: crate::Scope::UserOrAbove,
    },
    Setting {
        key: "agents.detection",
        kind: Kind::Enumerated,
        default: "\"all\"",
        doc: "Which agent-detection sources to run.",
        choices: &["all", "told-only", "off"],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.foreground",
        kind: Kind::Text,
        default: "\"\"",
        doc: "What to answer a program that asks what colour the text is, as `rgb:rrrr/gggg/bbbb`. Empty means omt says nothing rather than guessing.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "terminal.background",
        kind: Kind::Text,
        default: "\"\"",
        doc: "The same for the background — the one agent CLIs read to decide whether they are on a light or a dark terminal.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "notifications.sound",
        kind: Kind::Bool,
        default: "true",
        doc: "Whether a notification also makes a noise. omt ships no audio and plays whatever this machine already has.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "notifications.sound.needs_you",
        kind: Kind::Text,
        default: "\"\"",
        doc: "A sound file to play when an agent starts waiting for a person. Empty means the system's own.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "notifications.sound.finished",
        kind: Kind::Text,
        default: "\"\"",
        doc: "A sound file to play when an agent finishes while nobody is watching. Empty means the system's own.",
        choices: &[],
        scope: crate::Scope::Any,
    },
    Setting {
        key: "notifications.when",
        kind: Kind::Enumerated,
        default: "\"needs-you\"",
        doc: "When to notify. `needs-you` is a card waiting on a human, which is the only one worth a buzz by default.",
        choices: &["needs-you", "any-change", "off"],
        scope: crate::Scope::Any,
    },
];

/// One setting by key.
#[must_use]
pub fn setting(key: &str) -> Option<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key)
}

/// A problem found while checking a value against its descriptor.
///
/// `docs/architecture/10-configuration.md` §5.1. Every pass runs to completion
/// collecting these; none aborts on the first problem. Somebody who mistyped
/// four settings wants to be told about four, not to fix one, restart, and
/// find the next — which is four restarts to learn what one run could have
/// said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Which key.
    pub key: String,
    /// What is wrong.
    pub message: String,
    /// What to do about it.
    pub help: String,
}

/// What kind of thing a JSON value is, in the descriptor's vocabulary.
fn kind_of(value: &serde_json::Value) -> Option<Kind> {
    match value {
        serde_json::Value::Bool(_) => Some(Kind::Bool),
        serde_json::Value::Number(n) => {
            if n.is_f64() && n.as_i64().is_none() {
                Some(Kind::Number)
            } else {
                Some(Kind::Integer)
            }
        }
        serde_json::Value::String(_) => Some(Kind::Text),
        serde_json::Value::Array(_) => Some(Kind::List),
        _ => None,
    }
}

/// Whether a value of this kind can be used where that kind is wanted.
///
/// An integer is acceptable where a decimal is wanted, because `1` and `1.0`
/// are the same number and refusing the first would fail a config that is
/// correct in every way somebody can see. The reverse is not true: silently
/// truncating `1.5` to `1` would apply a setting nobody wrote.
const fn accepts(wanted: Kind, found: Kind) -> bool {
    matches!(
        (wanted, found),
        (Kind::Bool, Kind::Bool)
            | (Kind::Integer, Kind::Integer)
            | (Kind::Number, Kind::Number | Kind::Integer)
            | (Kind::Text | Kind::Enumerated, Kind::Text)
            | (Kind::List, Kind::List)
    )
}

/// Check flattened configuration values against the descriptors.
///
/// Returns every problem, in key order. Ordered so two runs over the same file
/// report the same thing in the same sequence — a diagnostic list that shuffles
/// is one nobody can diff between runs.
#[must_use]
pub fn validate(values: &std::collections::BTreeMap<String, serde_json::Value>) -> Vec<Problem> {
    let by_key: std::collections::BTreeMap<&str, &Setting> =
        SETTINGS.iter().map(|s| (s.key, s)).collect();
    let mut problems = Vec::new();

    for (key, value) in values {
        let Some(setting) = by_key.get(key.as_str()) else {
            // Unknown keys are the merge pass's business; reporting them twice
            // would have somebody fix one message and still see the other.
            continue;
        };

        let Some(found) = kind_of(value) else {
            problems.push(Problem {
                key: key.clone(),
                message: format!("`{key}` cannot hold a {}", shape_name(value)),
                help: format!("it holds {}", setting.kind.as_str()),
            });
            continue;
        };

        if !accepts(setting.kind, found) {
            problems.push(Problem {
                key: key.clone(),
                message: format!(
                    "`{key}` holds {} but a {} was written",
                    setting.kind.as_str(),
                    found.as_str()
                ),
                help: format!("write a {} value", setting.kind.as_str()),
            });
            continue;
        }

        if setting.kind == Kind::Enumerated
            && let Some(text) = value.as_str()
            && !setting.choices.contains(&text)
        {
            problems.push(Problem {
                key: key.clone(),
                message: format!("`{text}` is not one of the values `{key}` accepts"),
                // The choices, listed. "Invalid value" with the valid ones
                // withheld is a message that sends somebody to the source.
                help: format!("it accepts {}", setting.choices.join(", ")),
            });
        }
    }

    problems
}

/// What to call a value that is not a scalar.
fn shape_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Array(_) => "list",
        serde_json::Value::Object(_) => "table",
        serde_json::Value::Null => "nothing",
        _ => "value",
    }
}

/// When a change to a setting takes effect.
///
/// `docs/architecture/10-configuration.md` §6.3. The class matters because a
/// setting that quietly needs a restart is the worst of the three: somebody
/// changes it, nothing happens, and they conclude the setting does not work
/// rather than that it is waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reload {
    /// Applied immediately, visible in every attached surface.
    Hot,
    /// The listener is rebuilt; clients reconnect, PTYs are untouched.
    RestartServer,
    /// Recorded as pending until the process restarts.
    Restart,
}

/// When a change to this key takes effect.
///
/// Derived from the key's own prefix rather than listed per setting. A list
/// would need an entry for every new setting and would silently default to
/// `Hot` for the ones somebody forgot — which is exactly the case that
/// produces "I changed it and nothing happened".
#[must_use]
pub fn reload_class(key: &str) -> Reload {
    // The store and the parser are built once at startup and handed to
    // everything else; swapping either under a running session is not a
    // reload, it is a different program.
    const NEEDS_PROCESS: &[&str] = &["store.", "terminal.vt.", "media.tmp_dir"];
    // The listener can be rebuilt without touching a PTY, which is why this is
    // its own class rather than folded into a full restart: losing a session
    // to change a TLS certificate would be a poor trade.
    const NEEDS_LISTENER: &[&str] = &[
        "server.bind",
        "server.tls",
        "server.auth",
        "server.tailscale",
    ];

    if NEEDS_PROCESS.iter().any(|p| key.starts_with(p)) {
        Reload::Restart
    } else if NEEDS_LISTENER.iter().any(|p| key.starts_with(p)) {
        Reload::RestartServer
    } else {
        Reload::Hot
    }
}

/// Whether changing this key loses something the user cannot get back.
///
/// Only shrinking scrollback, today. Reported so the surface that initiated
/// the change can ask first — a setting that silently discards a day of output
/// is one somebody adjusts once and never trusts again.
#[must_use]
pub fn is_lossy(key: &str, before: Option<i64>, after: Option<i64>) -> bool {
    key == "terminal.scrollback_lines" && matches!((before, after), (Some(b), Some(a)) if a < b)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_has_a_default() {
        // P11: a feature works before it is configured. A setting with no
        // default is one that must be set, which is the same failure with an
        // extra step in front of it.
        for s in SETTINGS {
            assert!(!s.default.is_empty(), "{} has no default", s.key);
        }
    }

    #[test]
    fn every_default_is_valid_json_of_its_declared_kind() {
        // A default that does not parse is a default nothing can apply, and it
        // would surface as a setting that mysteriously has no value.
        for s in SETTINGS {
            let value: serde_json::Value = serde_json::from_str(s.default)
                .unwrap_or_else(|e| panic!("{} has an unparseable default: {e}", s.key));
            let ok = match s.kind {
                Kind::Bool => value.is_boolean(),
                Kind::Integer => value.is_i64() || value.is_u64(),
                Kind::Number => value.is_number(),
                Kind::Text | Kind::Enumerated => value.is_string(),
                Kind::List => value.is_array(),
            };
            assert!(
                ok,
                "{} declares {:?} and defaults to {value}",
                s.key, s.kind
            );
        }
    }

    #[test]
    fn an_enumerated_setting_defaults_to_one_of_its_choices() {
        // Otherwise the shipped configuration is invalid against its own
        // schema, which every validator would then report on a fresh install.
        for s in SETTINGS.iter().filter(|s| s.kind == Kind::Enumerated) {
            assert!(!s.choices.is_empty(), "{} lists no choices", s.key);
            let default: String =
                serde_json::from_str(s.default).expect("an enum default is a string");
            assert!(
                s.choices.contains(&default.as_str()),
                "{} defaults to {default}, which is not one of {:?}",
                s.key,
                s.choices
            );
        }
    }

    #[test]
    fn only_enumerated_settings_list_choices() {
        // Choices on a free-text setting suggest a constraint that nothing
        // enforces.
        for s in SETTINGS.iter().filter(|s| s.kind != Kind::Enumerated) {
            assert!(
                s.choices.is_empty(),
                "{} lists choices but is not an enum",
                s.key
            );
        }
    }

    #[test]
    fn every_setting_says_what_it_does() {
        // A settings screen listing a key with no description is a list of
        // names to guess at.
        for s in SETTINGS {
            assert!(s.doc.len() > 10, "{} has no useful description", s.key);
        }
    }

    #[test]
    fn keys_are_unique_and_dotted() {
        let mut seen = std::collections::BTreeSet::new();
        for s in SETTINGS {
            assert!(seen.insert(s.key), "{} is declared twice", s.key);
            assert!(s.key.contains('.'), "{} is not namespaced", s.key);
        }
    }

    #[test]
    fn a_setting_can_be_found_by_key() {
        assert!(setting("font.size").is_some());
        assert!(setting("font.nonexistent").is_none());
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod validation_tests {
    use super::*;

    fn values(
        pairs: &[(&str, serde_json::Value)],
    ) -> std::collections::BTreeMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn a_setting_of(kind: Kind) -> &'static Setting {
        SETTINGS
            .iter()
            .find(|s| s.kind == kind)
            .unwrap_or_else(|| panic!("a {kind:?} setting"))
    }

    #[test]
    fn a_correct_configuration_reports_nothing() {
        // A validator that complains about a working config is one people stop
        // reading.
        let setting = a_setting_of(Kind::Bool);
        assert!(validate(&values(&[(setting.key, serde_json::json!(true))])).is_empty());
    }

    #[test]
    fn a_value_of_the_wrong_type_is_reported_with_both_types() {
        // "Type mismatch" alone leaves somebody guessing which way round.
        let setting = a_setting_of(Kind::Bool);
        let found = validate(&values(&[(setting.key, serde_json::json!("yes"))]));
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("bool"), "{}", found[0].message);
        assert!(found[0].message.contains("text"), "{}", found[0].message);
    }

    #[test]
    fn an_integer_is_accepted_where_a_decimal_is_wanted() {
        // `1` and `1.0` are the same number, and refusing the first fails a
        // config that is correct in every way somebody can see.
        let Some(setting) = SETTINGS.iter().find(|s| s.kind == Kind::Number) else {
            return;
        };
        assert!(validate(&values(&[(setting.key, serde_json::json!(1))])).is_empty());
    }

    #[test]
    fn a_decimal_is_not_quietly_truncated_into_an_integer() {
        // Truncating `1.5` to `1` applies a setting nobody wrote.
        let setting = a_setting_of(Kind::Integer);
        assert_eq!(
            validate(&values(&[(setting.key, serde_json::json!(1.5))])).len(),
            1
        );
    }

    #[test]
    fn a_value_outside_an_enumeration_lists_the_ones_that_work() {
        // "Invalid value" with the valid ones withheld is a message that sends
        // somebody to the source.
        let Some(setting) = SETTINGS.iter().find(|s| s.kind == Kind::Enumerated) else {
            return;
        };
        let found = validate(&values(&[(setting.key, serde_json::json!("nonsense"))]));
        assert_eq!(found.len(), 1, "{found:?}");
        for choice in setting.choices {
            assert!(found[0].help.contains(choice), "{}", found[0].help);
        }
    }

    #[test]
    fn a_permitted_enumeration_value_is_accepted() {
        let Some(setting) = SETTINGS.iter().find(|s| s.kind == Kind::Enumerated) else {
            return;
        };
        let choice = setting.choices.first().expect("a choice");
        assert!(validate(&values(&[(setting.key, serde_json::json!(choice))])).is_empty());
    }

    #[test]
    fn a_table_where_a_scalar_belongs_is_named_as_a_table() {
        let setting = a_setting_of(Kind::Bool);
        let found = validate(&values(&[(setting.key, serde_json::json!({ "a": 1 }))]));
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("table"), "{}", found[0].message);
    }

    #[test]
    fn every_problem_in_the_file_is_reported_in_one_pass() {
        // Fixing one, restarting, and finding the next is four restarts to
        // learn what one run could have said.
        let a = a_setting_of(Kind::Bool);
        let b = a_setting_of(Kind::Integer);
        let found = validate(&values(&[
            (a.key, serde_json::json!("no")),
            (b.key, serde_json::json!("lots")),
        ]));
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    fn the_order_is_stable_between_runs() {
        // A diagnostic list that shuffles is one nobody can diff.
        let a = a_setting_of(Kind::Bool);
        let b = a_setting_of(Kind::Integer);
        let input = values(&[
            (a.key, serde_json::json!("no")),
            (b.key, serde_json::json!("lots")),
        ]);
        let once = validate(&input);
        for _ in 0..5 {
            assert_eq!(validate(&input), once);
        }
    }

    #[test]
    fn an_unknown_key_is_left_to_the_merge_pass() {
        // Reporting it twice has somebody fix one message and still see the
        // other.
        assert!(validate(&values(&[("nothing.at.all", serde_json::json!(1))])).is_empty());
    }

    #[test]
    fn every_problem_carries_help() {
        let setting = a_setting_of(Kind::Bool);
        for problem in validate(&values(&[(setting.key, serde_json::json!(7))])) {
            assert!(!problem.help.is_empty(), "{}", problem.key);
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod reload_tests {
    use super::*;

    #[test]
    fn appearance_and_keys_apply_immediately() {
        // The whole point of a config file somebody edits while looking at the
        // thing it configures.
        for key in ["ui.theme", "keys.leader", "agent.detect", "log.level"] {
            assert_eq!(reload_class(key), Reload::Hot, "{key}");
        }
    }

    #[test]
    fn the_listener_settings_rebuild_the_listener_and_nothing_else() {
        // Losing a session to change a TLS certificate would be a poor trade,
        // which is why this is its own class rather than a full restart.
        for key in ["server.bind", "server.tls.cert", "server.auth.backends"] {
            assert_eq!(reload_class(key), Reload::RestartServer, "{key}");
        }
    }

    #[test]
    fn the_store_and_the_parser_need_the_process_back() {
        // Both are built once and handed to everything else; swapping either
        // under a running session is not a reload, it is a different program.
        for key in [
            "store.backend",
            "store.path",
            "terminal.vt.strict",
            "media.tmp_dir",
        ] {
            assert_eq!(reload_class(key), Reload::Restart, "{key}");
        }
    }

    #[test]
    fn an_unknown_key_is_hot_rather_than_silently_pending() {
        // Derived from the prefix rather than listed, because a list defaults
        // to Hot for whatever somebody forgot to add — and "I changed it and
        // nothing happened" is the worst of the three outcomes.
        assert_eq!(reload_class("some.new.setting"), Reload::Hot);
    }

    #[test]
    fn every_setting_omt_ships_has_a_class() {
        for setting in SETTINGS {
            let _ = reload_class(setting.key);
        }
    }

    #[test]
    fn shrinking_the_scrollback_is_lossy_and_growing_it_is_not() {
        // A setting that silently discards a day of output is one somebody
        // adjusts once and never trusts again.
        assert!(is_lossy(
            "terminal.scrollback_lines",
            Some(10_000),
            Some(1_000)
        ));
        assert!(!is_lossy(
            "terminal.scrollback_lines",
            Some(1_000),
            Some(10_000)
        ));
        assert!(!is_lossy(
            "terminal.scrollback_lines",
            Some(1_000),
            Some(1_000)
        ));
    }

    #[test]
    fn nothing_else_is_lossy() {
        assert!(!is_lossy("ui.theme", Some(10), Some(1)));
    }
}

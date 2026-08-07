//! Reading the themes people already have.
//!
//! "Bring your own colours" is worth more than "here are ours". Every importer
//! here is lossy in one direction only: it takes what maps onto a terminal
//! palette and reports what it could not use, rather than silently substituting
//! — a theme that quietly lost half its colours looks like omt rendering badly.

use crate::{Appearance, Palette, Rgb, Theme};

/// Why an import failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    /// The file was not the format it claimed.
    #[error("that is not a {format} theme: {detail}")]
    WrongFormat {
        /// Which format was expected.
        format: &'static str,
        /// What went wrong.
        detail: String,
    },
    /// It parsed but had no colours omt can use.
    #[error("no terminal colours found in that {format} theme")]
    NothingUsable {
        /// Which format.
        format: &'static str,
    },
}

/// What an import produced, and what it could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    /// The theme.
    pub theme: Theme,
    /// Keys the source had that omt has nowhere to put.
    ///
    /// Reported rather than dropped: a user who spent an hour on a VS Code
    /// theme should be told which parts a terminal cannot show, instead of
    /// wondering why it looks different.
    pub unmapped: Vec<String>,
}

/// Import a YAML theme.
///
/// These themes are YAML with `accent`, `background`, `foreground`, `details`,
/// and a `terminal_colors` block of `normal` and `bright`.
///
/// # Errors
/// Fails if the text is not a The YAML theme format theme or carries no usable colours.
pub fn from_yaml(name: &str, text: &str) -> Result<Imported, ImportError> {
    // A small YAML reader rather than a dependency: the subset The YAML theme format themes use
    // is `key: value` and one level of nesting, and pulling in a YAML parser
    // for that would be a supply-chain decision made for twenty lines.
    let map = parse_simple_yaml(text);
    let mut unmapped = Vec::new();

    let get = |key: &str| map.get(key).and_then(|v| Rgb::parse(v));
    let background = get("background").ok_or(ImportError::NothingUsable { format: "The YAML theme format" })?;
    let foreground = get("foreground").ok_or(ImportError::NothingUsable { format: "The YAML theme format" })?;

    let colour_at = |prefix: &str, index: usize| {
        const NAMES: [&str; 8] = [
            "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
        ];
        map.get(&format!("{prefix}.{}", NAMES[index]))
            .and_then(|v| Rgb::parse(v))
            .unwrap_or(foreground)
    };

    let mut normal = [foreground; 8];
    let mut bright = [foreground; 8];
    for i in 0..8 {
        normal[i] = colour_at("normal", i);
        bright[i] = colour_at("bright", i);
    }

    for key in map.keys() {
        // The YAML theme format themes carry a background image and gradients; a terminal
        // palette has nowhere for either.
        if key.starts_with("background_image") || key.contains("gradient") {
            unmapped.push(key.clone());
        }
    }

    let appearance = if map.get("details").map(String::as_str) == Some("lighter") {
        Appearance::Light
    } else if background.luminance() < 0.5 {
        Appearance::Dark
    } else {
        Appearance::Light
    };

    Ok(Imported {
        theme: Theme {
            name: name.to_owned(),
            appearance,
            foreground,
            background,
            cursor: get("accent").unwrap_or(foreground),
            selection: get("accent").unwrap_or(foreground),
            palette: Palette { normal, bright },
        },
        unmapped,
    })
}

/// Import a VS Code theme.
///
/// VS Code themes carry hundreds of editor keys and a handful of terminal ones.
/// Only `terminal.*` and `editor.background`/`foreground` map onto a terminal;
/// the rest is reported as unmapped, which is most of the file and should be.
///
/// # Errors
/// Fails if the JSON does not parse or carries no terminal colours.
pub fn from_vscode(name: &str, text: &str) -> Result<Imported, ImportError> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ImportError::WrongFormat {
            format: "VS Code",
            detail: e.to_string(),
        })?;
    let colors =
        root.get("colors")
            .and_then(|c| c.as_object())
            .ok_or(ImportError::WrongFormat {
                format: "VS Code",
                detail: "no `colors` object".to_owned(),
            })?;

    let get = |key: &str| {
        colors
            .get(key)
            .and_then(|v| v.as_str())
            // VS Code allows an alpha channel that a terminal palette has no
            // room for. Truncating to the RGB is closer than refusing.
            .and_then(|v| Rgb::parse(v.get(..7).unwrap_or(v)))
    };

    let background = get("terminal.background")
        .or_else(|| get("editor.background"))
        .ok_or(ImportError::NothingUsable { format: "VS Code" })?;
    let foreground = get("terminal.foreground")
        .or_else(|| get("editor.foreground"))
        .ok_or(ImportError::NothingUsable { format: "VS Code" })?;

    const NAMES: [&str; 8] = [
        "Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White",
    ];
    let mut normal = [foreground; 8];
    let mut bright = [foreground; 8];
    for (i, n) in NAMES.iter().enumerate() {
        normal[i] = get(&format!("terminal.ansi{n}")).unwrap_or(normal[i]);
        bright[i] = get(&format!("terminal.ansiBright{n}")).unwrap_or(bright[i]);
    }

    // Only the keys actually consumed count as mapped. Excluding every
    // `editor.*` would under-report: two of them are read as fallbacks and the
    // other several hundred are genuinely things a terminal cannot show, which
    // is exactly what the user wants to be told.
    const CONSUMED_EDITOR: [&str; 2] = ["editor.background", "editor.foreground"];
    let unmapped: Vec<String> = colors
        .keys()
        .filter(|k| !k.starts_with("terminal") && !CONSUMED_EDITOR.contains(&k.as_str()))
        .cloned()
        .collect();

    let declared_dark = root
        .get("type")
        .and_then(|t| t.as_str())
        .map(|t| t.eq_ignore_ascii_case("dark"));
    let appearance = match declared_dark {
        // The theme's own declaration wins over a guess from the background:
        // its author knows, and a borderline background guesses wrong.
        Some(true) => Appearance::Dark,
        Some(false) => Appearance::Light,
        None if background.luminance() < 0.5 => Appearance::Dark,
        None => Appearance::Light,
    };

    Ok(Imported {
        theme: Theme {
            name: name.to_owned(),
            appearance,
            foreground,
            background,
            cursor: get("terminalCursor.foreground").unwrap_or(foreground),
            selection: get("terminal.selectionBackground").unwrap_or(background),
            palette: Palette { normal, bright },
        },
        unmapped,
    })
}

/// A reader for the YAML subset The YAML theme format themes use.
///
/// `key: value` and one level of nesting. Deliberately not a YAML parser:
/// pulling in a general one for twenty lines of format is a supply-chain
/// decision that should be made for a better reason.
fn parse_simple_yaml(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    // A stack of (indent, key), because The YAML theme format nests two levels —
    // `terminal_colors: normal: black:` — and a reader that tracked one would
    // flatten `normal.black` and `bright.black` onto each other, which is the
    // one collision that matters here.
    let mut stack: Vec<(usize, String)> = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches('"');
        let value = value.trim().trim_matches('"').trim_matches('\'');

        stack.retain(|(depth, _)| *depth < indent);

        if value.is_empty() {
            stack.push((indent, key.to_owned()));
            continue;
        }

        // The *nearest* enclosing section, not the whole path: The YAML theme format writes
        // `terminal_colors.normal.black` and every consumer wants
        // `normal.black`.
        let full = match stack.last() {
            Some((_, section)) => format!("{section}.{key}"),
            None => key.to_owned(),
        };
        out.insert(full, value.to_owned());
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    const YAML_THEME: &str = r##"
accent: "#268bd2"
background: "#002b36"
foreground: "#839496"
details: darker
terminal_colors:
  normal:
    black: "#073642"
    red: "#dc322f"
    green: "#859900"
    yellow: "#b58900"
    blue: "#268bd2"
    magenta: "#d33682"
    cyan: "#2aa198"
    white: "#eee8d5"
  bright:
    black: "#002b36"
    red: "#cb4b16"
    green: "#586e75"
    yellow: "#657b83"
    blue: "#839496"
    magenta: "#6c71c4"
    cyan: "#93a1a1"
    white: "#fdf6e3"
"##;

    const VSCODE: &str = r##"{
  "name": "Test Dark",
  "type": "dark",
  "colors": {
    "editor.background": "#1e1e1e",
    "editor.foreground": "#d4d4d4",
    "terminal.background": "#181818",
    "terminal.foreground": "#cccccc",
    "terminal.ansiRed": "#cd3131",
    "terminal.ansiBrightRed": "#f14c4c",
    "terminalCursor.foreground": "#ffffff",
    "activityBar.background": "#333333",
    "statusBar.background": "#007acc"
  }
}"##;

    #[test]
    fn a_yaml_theme_imports_its_palette() {
        let imported = from_yaml("solarized", YAML_THEME).expect("import");
        assert_eq!(imported.theme.background, Rgb::parse("#002b36").expect("c"));
        assert_eq!(
            imported.theme.palette.normal[1],
            Rgb::parse("#dc322f").expect("c")
        );
        assert_eq!(
            imported.theme.palette.bright[7],
            Rgb::parse("#fdf6e3").expect("c")
        );
        assert_eq!(imported.theme.appearance, Appearance::Dark);
    }

    #[test]
    fn a_yaml_theme_with_no_colours_is_refused_rather_than_defaulted() {
        // Producing a black-on-black theme from an unrelated file would look
        // like omt rendering badly.
        let err = from_yaml("empty", "name: nothing\n").expect_err("must refuse");
        assert!(matches!(err, ImportError::NothingUsable { .. }), "{err:?}");
    }

    #[test]
    fn a_vscode_theme_takes_its_terminal_colours() {
        let imported = from_vscode("test", VSCODE).expect("import");
        assert_eq!(imported.theme.background, Rgb::parse("#181818").expect("c"));
        assert_eq!(
            imported.theme.palette.normal[1],
            Rgb::parse("#cd3131").expect("c")
        );
        assert_eq!(
            imported.theme.palette.bright[1],
            Rgb::parse("#f14c4c").expect("c")
        );
    }

    #[test]
    fn a_vscode_theme_falls_back_to_the_editor_colours() {
        // Plenty of themes never set terminal.background, and refusing them
        // would reject most of the ecosystem.
        let text = r##"{"colors":{"editor.background":"#000000","editor.foreground":"#ffffff"}}"##;
        let imported = from_vscode("minimal", text).expect("import");
        assert_eq!(imported.theme.background, Rgb(0, 0, 0));
    }

    #[test]
    fn what_a_terminal_cannot_show_is_reported_rather_than_dropped() {
        // A user who spent an hour on a theme should be told which parts a
        // terminal cannot show, not left wondering why it looks different.
        let imported = from_vscode("test", VSCODE).expect("import");
        assert!(
            imported
                .unmapped
                .contains(&"activityBar.background".to_owned()),
            "{:?}",
            imported.unmapped
        );
        assert!(
            !imported.unmapped.iter().any(|k| k.starts_with("terminal.")),
            "a mapped key was reported as unmapped"
        );
    }

    #[test]
    fn an_editor_key_that_is_not_consumed_is_reported() {
        // Excluding every `editor.*` under-reports: two are read as fallbacks
        // and the rest are genuinely things a terminal cannot show.
        let text = r##"{"colors":{
            "editor.background":"#000000",
            "editor.foreground":"#ffffff",
            "editor.lineHighlightBackground":"#222222"
        }}"##;
        let imported = from_vscode("x", text).expect("import");
        assert_eq!(imported.unmapped, ["editor.lineHighlightBackground"]);
    }

    #[test]
    fn a_declared_appearance_beats_a_guess_from_the_background() {
        // The author knows; a borderline background guesses wrong.
        let text = r##"{"type":"light","colors":{"editor.background":"#3a3a3a","editor.foreground":"#000000"}}"##;
        let imported = from_vscode("odd", text).expect("import");
        assert_eq!(imported.theme.appearance, Appearance::Light);
    }

    #[test]
    fn an_alpha_channel_is_truncated_rather_than_refused() {
        // VS Code allows eight-digit colours and a terminal palette has no room
        // for alpha; refusing would reject a large part of the ecosystem.
        let text =
            r##"{"colors":{"editor.background":"#1e1e1eff","editor.foreground":"#d4d4d4cc"}}"##;
        let imported = from_vscode("alpha", text).expect("import");
        assert_eq!(imported.theme.background, Rgb::parse("#1e1e1e").expect("c"));
    }

    #[test]
    fn a_file_that_is_not_json_is_refused_with_a_reason() {
        let err = from_vscode("broken", "{ not json").expect_err("must refuse");
        assert!(matches!(err, ImportError::WrongFormat { .. }), "{err:?}");
    }

    #[test]
    fn an_imported_theme_is_checked_for_readability_like_any_other() {
        // Import is not a way around the warnings.
        let text = r##"{"colors":{"editor.background":"#2a2a2a","editor.foreground":"#333333"}}"##;
        let imported = from_vscode("dim", text).expect("import");
        assert!(!imported.theme.is_readable());
        assert!(!imported.theme.warnings().is_empty());
    }

    #[test]
    fn the_yaml_reader_handles_nesting_and_comments() {
        let map = parse_simple_yaml("# a comment\ntop: 1\nsection:\n  inner: 2\nafter: 3\n");
        assert_eq!(map.get("top").map(String::as_str), Some("1"));
        assert_eq!(map.get("section.inner").map(String::as_str), Some("2"));
        assert_eq!(map.get("after").map(String::as_str), Some("3"));
    }

    #[test]
    fn two_levels_of_nesting_do_not_collide() {
        // The YAML theme format writes `terminal_colors: normal: black:` and `bright: black:`.
        // A reader tracking one level flattens them onto each other, and the
        // bright palette silently becomes the normal one.
        let map = parse_simple_yaml(
            "terminal_colors:\n  normal:\n    black: \"#111111\"\n  bright:\n    black: \"#222222\"\n",
        );
        assert_eq!(map.get("normal.black").map(String::as_str), Some("#111111"));
        assert_eq!(map.get("bright.black").map(String::as_str), Some("#222222"));
    }
}

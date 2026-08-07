//! Themes and fonts.
//!
//! The format is the sixteen ANSI colours plus the handful of terminal-wide
//! ones, which is what every terminal emulator has agreed on for decades. omt
//! adds nothing to it, deliberately: a theme that only omt can read is a theme
//! nobody ports, and the point of a theme format is that people share them.
//!
//! Importers exist for the formats people already have — the YAML format's YAML, VS Code's
//! JSON, and iTerm2's plist — because "bring your own colours" is worth more
//! than "here are ours".

pub mod import;

pub use import::{ImportError, from_vscode, from_yaml};

use serde::{Deserialize, Serialize};

/// A colour, as `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Parse `#rgb`, `#rrggbb`, or the same without the hash.
    ///
    /// All three spellings, because a user pasting a colour from anywhere gets
    /// one of them and being strict here just makes the config refuse a value
    /// that is obviously a colour.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        match hex.len() {
            3 => {
                let expand = |c: char| {
                    let v = c.to_digit(16)? as u8;
                    Some(v * 17)
                };
                let mut chars = hex.chars();
                Some(Self(
                    expand(chars.next()?)?,
                    expand(chars.next()?)?,
                    expand(chars.next()?)?,
                ))
            }
            6 => Some(Self(
                u8::from_str_radix(hex.get(0..2)?, 16).ok()?,
                u8::from_str_radix(hex.get(2..4)?, 16).ok()?,
                u8::from_str_radix(hex.get(4..6)?, 16).ok()?,
            )),
            _ => None,
        }
    }

    /// The `#rrggbb` form.
    #[must_use]
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Relative luminance, for contrast checks.
    #[must_use]
    pub fn luminance(self) -> f64 {
        let channel = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.0722f64.mul_add(
            channel(self.2),
            0.2126f64.mul_add(channel(self.0), 0.7152 * channel(self.1)),
        )
    }

    /// The WCAG contrast ratio against another colour.
    ///
    /// Used to warn about a theme whose foreground and background are too close
    /// — which happens constantly when a light theme is imported into a dark
    /// terminal and nobody checks.
    #[must_use]
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (light, dark) = if a > b { (a, b) } else { (b, a) };
        (light + 0.05) / (dark + 0.05)
    }
}

impl TryFrom<String> for Rgb {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| format!("`{value}` is not a colour"))
    }
}

impl From<Rgb> for String {
    fn from(value: Rgb) -> Self {
        value.to_hex()
    }
}

/// The sixteen ANSI colours, in their conventional order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    /// 0–7.
    pub normal: [Rgb; 8],
    /// 8–15.
    pub bright: [Rgb; 8],
}

/// A complete theme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// What to call it.
    pub name: String,
    /// Whether it is meant for a light or dark terminal.
    ///
    /// Stated rather than inferred from the background: a theme's author knows,
    /// and guessing gets borderline themes wrong in a way that then propagates
    /// into every contrast decision.
    pub appearance: Appearance,
    /// Default text.
    pub foreground: Rgb,
    /// The terminal's background.
    pub background: Rgb,
    /// The cursor.
    pub cursor: Rgb,
    /// Selected text's background.
    pub selection: Rgb,
    /// The sixteen.
    pub palette: Palette,
}

impl Theme {
    /// The theme omt ships.
    ///
    /// One built in, rather than requiring an import before anything has
    /// colours: a terminal whose first run depends on the user finding a theme
    /// file is one that looks broken until they do. These are the same values
    /// the web client falls back to, so a session looks the same in a browser
    /// as it does locally.
    #[must_use]
    pub fn builtin_dark() -> Self {
        Self {
            name: "omt dark".to_owned(),
            appearance: Appearance::Dark,
            foreground: Rgb(0xe0, 0xe0, 0xe0),
            background: Rgb(0x10, 0x10, 0x10),
            cursor: Rgb(0xff, 0xb4, 0x54),
            selection: Rgb(0x33, 0x3a, 0x45),
            palette: Palette {
                normal: [
                    Rgb(0x10, 0x10, 0x10),
                    Rgb(0xe0, 0x52, 0x52),
                    Rgb(0x6c, 0xc0, 0x70),
                    Rgb(0xd7, 0xba, 0x7d),
                    Rgb(0x59, 0xa5, 0xff),
                    Rgb(0xc5, 0x86, 0xc0),
                    Rgb(0x4e, 0xc9, 0xb0),
                    Rgb(0xd0, 0xd0, 0xd0),
                ],
                bright: [
                    Rgb(0x6a, 0x6a, 0x6a),
                    Rgb(0xff, 0x7b, 0x72),
                    Rgb(0x8a, 0xde, 0x94),
                    Rgb(0xff, 0xd4, 0x79),
                    Rgb(0x8a, 0xb4, 0xff),
                    Rgb(0xdd, 0xa0, 0xdd),
                    Rgb(0x7f, 0xdb, 0xca),
                    Rgb(0xff, 0xff, 0xff),
                ],
            },
        }
    }
}

/// Whether a theme is light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    /// Dark background.
    Dark,
    /// Light background.
    Light,
}

/// The lowest contrast ratio a theme should have between text and background.
///
/// WCAG AA for body text. Below this, a theme is not unusable — people choose
/// low-contrast themes on purpose — but it is worth saying so once rather than
/// letting somebody wonder why their terminal is hard to read.
pub const MIN_CONTRAST: f64 = 4.5;

impl Theme {
    /// Whether the default text is readable on the default background.
    #[must_use]
    pub fn is_readable(&self) -> bool {
        self.foreground.contrast(self.background) >= MIN_CONTRAST
    }

    /// Anything worth telling the user about this theme.
    ///
    /// Warnings, never refusals. A theme is somebody's taste, and refusing to
    /// load one because a checker disliked it would be omt overruling the user
    /// about their own screen.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        let contrast = self.foreground.contrast(self.background);
        if contrast < MIN_CONTRAST {
            out.push(format!(
                "text-to-background contrast is {contrast:.1}:1, below the {MIN_CONTRAST}:1 \
                 usually considered readable"
            ));
        }
        if self.cursor.contrast(self.background) < 1.5 {
            out.push("the cursor is nearly the same colour as the background".to_owned());
        }
        if self.selection.contrast(self.background) < 1.2 {
            out.push("selected text will be hard to tell from unselected".to_owned());
        }
        out
    }

    /// Whether the appearance the theme claims matches its background.
    ///
    /// A mismatch is usually an import that guessed, and it matters because
    /// every other surface keys its own light/dark decision off this.
    #[must_use]
    pub fn appearance_matches_background(&self) -> bool {
        let dark = self.background.luminance() < 0.5;
        matches!(
            (self.appearance, dark),
            (Appearance::Dark, true) | (Appearance::Light, false)
        )
    }
}

/// A font, in terms every terminal understands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontSettings {
    /// The family, e.g. `JetBrains Mono`.
    pub family: String,
    /// Points.
    pub size: f32,
    /// Multiplied against the font's natural line height.
    pub line_height: f32,
    /// Extra space between characters, in pixels.
    pub letter_spacing: f32,
    /// Whether to use the font's programming ligatures.
    ///
    /// Off by default. Ligatures turn `!=` into a glyph that does not look like
    /// what is in the file, and somebody debugging a comparison should see the
    /// characters they typed.
    pub ligatures: bool,
    /// Families to fall back to, in order.
    pub fallback: Vec<String>,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            family: "monospace".to_owned(),
            size: 13.0,
            line_height: 1.2,
            letter_spacing: 0.0,
            ligatures: false,
            fallback: vec![
                // Emoji and CJK are the two that break a terminal's alignment
                // when they fall back to something proportional.
                "Apple Color Emoji".to_owned(),
                "Noto Color Emoji".to_owned(),
                "Noto Sans CJK SC".to_owned(),
            ],
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    #[test]
    fn the_builtin_theme_is_readable() {
        // The theme omt ships must pass omt's own contrast rule. Shipping one
        // that fails the check the importer applies to everyone else would be
        // the clearest possible statement that the check does not matter.
        let theme = Theme::builtin_dark();
        assert!(theme.is_readable(), "{:?}", theme.warnings());
    }

    #[test]
    fn the_builtin_theme_says_what_it_is() {
        let theme = Theme::builtin_dark();
        assert!(theme.appearance_matches_background());
    }

    use super::*;

    fn theme(fg: &str, bg: &str) -> Theme {
        let c = |h: &str| Rgb::parse(h).expect("colour");
        Theme {
            name: "test".to_owned(),
            appearance: Appearance::Dark,
            foreground: c(fg),
            background: c(bg),
            cursor: c("#ffffff"),
            selection: c("#444444"),
            palette: Palette {
                normal: [c("#000000"); 8],
                bright: [c("#ffffff"); 8],
            },
        }
    }

    #[test]
    fn every_spelling_of_a_colour_parses() {
        // A user pasting a colour from anywhere gets one of these, and being
        // strict just refuses a value that is obviously a colour.
        assert_eq!(Rgb::parse("#ff8800"), Some(Rgb(255, 136, 0)));
        assert_eq!(Rgb::parse("ff8800"), Some(Rgb(255, 136, 0)));
        assert_eq!(Rgb::parse("#f80"), Some(Rgb(255, 136, 0)));
        assert_eq!(Rgb::parse("  #FF8800  "), Some(Rgb(255, 136, 0)));
    }

    #[test]
    fn a_non_colour_is_refused_rather_than_defaulted() {
        // Defaulting to black would give somebody an invisible theme with no
        // explanation.
        assert_eq!(Rgb::parse("blue"), None);
        assert_eq!(Rgb::parse("#12345"), None);
        assert_eq!(Rgb::parse(""), None);
    }

    #[test]
    fn a_colour_round_trips_through_its_hex() {
        let c = Rgb(18, 52, 86);
        assert_eq!(Rgb::parse(&c.to_hex()), Some(c));
    }

    #[test]
    fn contrast_is_symmetric_and_matches_known_values() {
        let black = Rgb(0, 0, 0);
        let white = Rgb(255, 255, 255);
        // The WCAG maximum.
        assert!((black.contrast(white) - 21.0).abs() < 0.1);
        assert!((white.contrast(black) - 21.0).abs() < 0.1);
        assert!((white.contrast(white) - 1.0).abs() < 0.01);
    }

    #[test]
    fn an_unreadable_theme_is_warned_about_not_refused() {
        // A theme is somebody's taste. Refusing one because a checker disliked
        // it would be omt overruling the user about their own screen.
        let bad = theme("#333333", "#2a2a2a");
        assert!(!bad.is_readable());
        assert!(
            bad.warnings().iter().any(|w| w.contains("contrast")),
            "{:?}",
            bad.warnings()
        );
    }

    #[test]
    fn a_readable_theme_produces_no_warnings() {
        let good = theme("#e0e0e0", "#101010");
        assert!(good.is_readable());
        assert!(good.warnings().is_empty(), "{:?}", good.warnings());
    }

    #[test]
    fn an_invisible_cursor_is_worth_saying() {
        let mut t = theme("#ffffff", "#000000");
        t.cursor = Rgb::parse("#010101").expect("colour");
        assert!(
            t.warnings().iter().any(|w| w.contains("cursor")),
            "{:?}",
            t.warnings()
        );
    }

    #[test]
    fn a_theme_that_lies_about_being_dark_is_detectable() {
        // Usually an import that guessed, and it matters because every other
        // surface keys its own light/dark decision off this.
        let mut t = theme("#000000", "#ffffff");
        t.appearance = Appearance::Dark;
        assert!(!t.appearance_matches_background());
        t.appearance = Appearance::Light;
        assert!(t.appearance_matches_background());
    }

    #[test]
    fn ligatures_are_off_by_default() {
        // They turn `!=` into a glyph that does not look like what is in the
        // file, and somebody debugging a comparison should see what they typed.
        assert!(!FontSettings::default().ligatures);
    }

    #[test]
    fn the_default_fallbacks_cover_what_breaks_alignment() {
        // Emoji and CJK are the two that wreck a terminal's grid when they fall
        // back to something proportional.
        let f = FontSettings::default();
        assert!(f.fallback.iter().any(|n| n.contains("Emoji")));
        assert!(f.fallback.iter().any(|n| n.contains("CJK")));
    }

    #[test]
    fn a_theme_round_trips_through_json() {
        // The format is what people share, so it has to survive a save and a
        // load without changing.
        let t = theme("#e0e0e0", "#101010");
        let text = serde_json::to_string(&t).expect("serialize");
        let back: Theme = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, t);
        assert!(text.contains("#e0e0e0"), "colours stay readable: {text}");
    }
}

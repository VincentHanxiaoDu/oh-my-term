//! The cell: sixteen bytes, copied constantly, compared even more often.

use bitflags::bitflags;

/// A colour, packed into one word.
///
/// The tag lives in the top two bits and the payload below it. Packing this way
/// rather than as separate mode/r/g/b fields costs nothing in space and buys a
/// single-word equality test — which is the hot operation, since damage
/// comparison and the renderer's run coalescing both do nothing else.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color(u32);

const TAG_SHIFT: u32 = 30;
const TAG_DEFAULT: u32 = 0;
const TAG_INDEXED: u32 = 1;
const TAG_RGB: u32 = 2;
const PAYLOAD: u32 = (1 << TAG_SHIFT) - 1;

impl Color {
    /// Whatever the renderer's palette says the default is.
    pub const DEFAULT: Self = Self(TAG_DEFAULT << TAG_SHIFT);

    /// A palette entry.
    #[must_use]
    pub const fn indexed(i: u8) -> Self {
        Self((TAG_INDEXED << TAG_SHIFT) | i as u32)
    }

    /// A direct colour.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self((TAG_RGB << TAG_SHIFT) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    /// What this colour actually is.
    #[must_use]
    pub const fn kind(self) -> ColorKind {
        match self.0 >> TAG_SHIFT {
            TAG_INDEXED => ColorKind::Indexed((self.0 & 0xff) as u8),
            TAG_RGB => ColorKind::Rgb(
                ((self.0 >> 16) & 0xff) as u8,
                ((self.0 >> 8) & 0xff) as u8,
                (self.0 & 0xff) as u8,
            ),
            // Any other tag is unreachable through the constructors, and
            // treating it as the default is the only harmless reading.
            _ => ColorKind::Default,
        }
    }

    /// Whether this is the palette default rather than a chosen colour.
    #[must_use]
    pub const fn is_default(self) -> bool {
        self.0 >> TAG_SHIFT == TAG_DEFAULT && (self.0 & PAYLOAD) == 0
    }
}

impl std::fmt::Debug for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind() {
            ColorKind::Default => f.write_str("Default"),
            ColorKind::Indexed(i) => write!(f, "Indexed({i})"),
            ColorKind::Rgb(r, g, b) => write!(f, "Rgb({r},{g},{b})"),
        }
    }
}

/// The unpacked form of a [`Color`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorKind {
    /// The renderer's default.
    Default,
    /// A palette index.
    Indexed(u8),
    /// A direct 24-bit colour.
    Rgb(u8, u8, u8),
}

/// How a cell is underlined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Underline {
    /// Not underlined.
    #[default]
    None = 0,
    /// A single line.
    Single = 1,
    /// Two lines.
    Double = 2,
    /// The wavy one spell checkers use.
    Curly = 3,
    /// Dots.
    Dotted = 4,
    /// Dashes.
    Dashed = 5,
}

impl Underline {
    /// Parse the SGR 4:n sub-parameter.
    #[must_use]
    pub const fn from_style(n: u8) -> Self {
        match n {
            1 => Self::Single,
            2 => Self::Double,
            3 => Self::Curly,
            4 => Self::Dotted,
            5 => Self::Dashed,
            _ => Self::None,
        }
    }
}

bitflags! {
    /// Everything about a cell that is not a character or a colour.
    ///
    /// The underline style is three contiguous bits rather than a bit per
    /// style: styles are mutually exclusive, so separate bits would make
    /// "curly and dotted" representable, and something would eventually have to
    /// decide what that means.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Flags: u16 {
        /// Bold.
        const BOLD = 1 << 0;
        /// Faint.
        const FAINT = 1 << 1;
        /// Italic.
        const ITALIC = 1 << 2;
        /// Blinking.
        const BLINK = 1 << 3;
        /// Foreground and background swapped.
        const INVERSE = 1 << 4;
        /// Drawn as blanks.
        const INVISIBLE = 1 << 5;
        /// Struck through.
        const STRIKETHROUGH = 1 << 6;
        /// The three-bit underline style field.
        const UNDERLINE_MASK = 0b111 << 7;
        /// `ch` is a grapheme id rather than a character.
        const COMPLEX = 1 << 10;
        /// Left half of a double-width cluster.
        const WIDE = 1 << 11;
        /// Right half of one, or wrap padding.
        const WIDE_SPACER = 1 << 12;
        /// Inside a DECSCA-guarded area.
        const PROTECTED = 1 << 13;
        /// Covered by an inline image placement.
        const IMAGE = 1 << 14;
    }
}

const UNDERLINE_SHIFT: u32 = 7;

impl Flags {
    /// The underline style encoded in the style field.
    #[must_use]
    pub const fn underline(self) -> Underline {
        Underline::from_style(
            ((self.bits() & Flags::UNDERLINE_MASK.bits()) >> UNDERLINE_SHIFT) as u8,
        )
    }

    /// Replace the underline style, leaving every other flag alone.
    #[must_use]
    pub const fn with_underline(self, u: Underline) -> Self {
        let cleared = self.bits() & !Flags::UNDERLINE_MASK.bits();
        Self::from_bits_retain(cleared | ((u as u16) << UNDERLINE_SHIFT))
    }
}

/// A cell's character, or a reference to a stored grapheme cluster.
///
/// Most cells hold one scalar. The rest — emoji with modifiers, combining
/// marks, flags — are interned, so the common case never allocates and the rare
/// case is not lost.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CharOrGrapheme(u32);

impl CharOrGrapheme {
    /// A single character.
    #[must_use]
    pub const fn from_char(c: char) -> Self {
        Self(c as u32)
    }

    /// A reference into the grapheme table.
    #[must_use]
    pub const fn from_grapheme(id: GraphemeId) -> Self {
        Self(id.0)
    }

    /// Read it back, given the flag that says which it is.
    #[must_use]
    pub const fn resolve(self, complex: bool) -> Resolved {
        if complex {
            Resolved::Grapheme(GraphemeId(self.0))
        } else {
            match char::from_u32(self.0) {
                Some(c) => Resolved::Char(c),
                // Only reachable if a grapheme id was stored without the flag.
                None => Resolved::Char('\u{fffd}'),
            }
        }
    }
}

/// The result of reading a cell's character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    /// One scalar.
    Char(char),
    /// A cluster held in the grapheme table.
    Grapheme(GraphemeId),
}

/// A handle into the grapheme table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphemeId(pub u32);

/// An index into a line's side table of runs; zero means none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ExtraId(pub u16);

/// One character position on the screen.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    ch: CharOrGrapheme,
    /// Foreground colour.
    pub fg: Color,
    /// Background colour.
    pub bg: Color,
    /// Style bits.
    pub flags: Flags,
    /// Reserved for a future per-cell side table; hyperlinks use runs instead.
    pub extra: ExtraId,
}

impl Cell {
    /// An empty cell with default colours.
    pub const BLANK: Self = Self {
        ch: CharOrGrapheme(' ' as u32),
        fg: Color::DEFAULT,
        bg: Color::DEFAULT,
        flags: Flags::empty(),
        extra: ExtraId(0),
    };

    /// A cell holding one character with the given style.
    #[must_use]
    pub const fn new(c: char, fg: Color, bg: Color, flags: Flags) -> Self {
        Self {
            ch: CharOrGrapheme::from_char(c),
            fg,
            bg,
            flags,
            extra: ExtraId(0),
        }
    }

    /// Point this cell at an interned cluster.
    #[must_use]
    pub const fn with_grapheme(mut self, id: GraphemeId) -> Self {
        self.ch = CharOrGrapheme::from_grapheme(id);
        self.flags = self.flags.union(Flags::COMPLEX);
        self
    }

    /// What this cell holds.
    #[must_use]
    pub const fn resolve(self) -> Resolved {
        self.ch.resolve(self.flags.contains(Flags::COMPLEX))
    }

    /// Whether this cell would render as nothing but its background.
    ///
    /// Used by line trimming, which is where most of the memory saving in the
    /// whole crate comes from.
    #[must_use]
    pub fn is_blank_with(self, template: Self) -> bool {
        matches!(self.resolve(), Resolved::Char(' ' | '\0'))
            && self.bg == template.bg
            && self.flags.difference(Flags::WIDE_SPACER).is_empty()
    }

    /// The wrap padding written when a double-width cluster will not fit in the
    /// last column.
    ///
    /// A distinct value rather than a blank: reflow must drop it rather than
    /// re-emit it, and that decision needs something to key on.
    #[must_use]
    pub const fn wrap_padding(bg: Color) -> Self {
        Self {
            ch: CharOrGrapheme('\0' as u32),
            fg: Color::DEFAULT,
            bg,
            flags: Flags::WIDE_SPACER,
            extra: ExtraId(0),
        }
    }

    /// Whether this is the padding above rather than a real character.
    #[must_use]
    pub fn is_wrap_padding(self) -> bool {
        self.flags.contains(Flags::WIDE_SPACER) && self.resolve() == Resolved::Char('\0')
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::BLANK
    }
}

impl std::fmt::Debug for Cell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("Cell");
        match self.resolve() {
            Resolved::Char(c) => d.field("ch", &c),
            Resolved::Grapheme(g) => d.field("grapheme", &g.0),
        };
        d.field("fg", &self.fg)
            .field("bg", &self.bg)
            .field("flags", &self.flags)
            .finish()
    }
}

// The whole memory argument for scrollback — and the claim that a cell is
// cheap to copy — rests on this number. Asserted at compile time so a field
// added without thinking fails the build rather than the budget.
const _: () = assert!(size_of::<Cell>() == 16);

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_is_sixteen_bytes() {
        assert_eq!(size_of::<Cell>(), 16);
        assert_eq!(align_of::<Cell>(), 4);
    }

    #[test]
    fn colours_round_trip_through_the_packed_form() {
        assert_eq!(Color::DEFAULT.kind(), ColorKind::Default);
        assert_eq!(Color::indexed(200).kind(), ColorKind::Indexed(200));
        assert_eq!(Color::rgb(1, 2, 3).kind(), ColorKind::Rgb(1, 2, 3));
        assert_eq!(
            Color::rgb(255, 255, 255).kind(),
            ColorKind::Rgb(255, 255, 255)
        );
    }

    #[test]
    fn colour_equality_is_one_word() {
        // The property the packing exists for: comparing two styled cells must
        // not walk fields.
        assert_eq!(size_of::<Color>(), 4);
        assert_eq!(Color::rgb(1, 2, 3), Color::rgb(1, 2, 3));
        assert_ne!(Color::rgb(1, 2, 3), Color::rgb(1, 2, 4));
        assert_ne!(Color::indexed(1), Color::rgb(0, 0, 1));
    }

    #[test]
    fn the_default_colour_is_distinguishable_from_black() {
        // A renderer must be able to tell "no colour chosen" from "chosen
        // black", because a theme change moves one and not the other.
        assert!(Color::DEFAULT.is_default());
        assert!(!Color::rgb(0, 0, 0).is_default());
        assert!(!Color::indexed(0).is_default());
    }

    #[test]
    fn underline_styles_are_mutually_exclusive() {
        let f = Flags::BOLD.with_underline(Underline::Curly);
        assert_eq!(f.underline(), Underline::Curly);
        assert!(f.contains(Flags::BOLD), "other flags survive");

        let f = f.with_underline(Underline::Dotted);
        assert_eq!(
            f.underline(),
            Underline::Dotted,
            "setting a style replaces rather than accumulates"
        );
        assert!(f.contains(Flags::BOLD));
    }

    #[test]
    fn clearing_the_underline_leaves_the_rest_alone() {
        let f = Flags::ITALIC
            .union(Flags::WIDE)
            .with_underline(Underline::Double)
            .with_underline(Underline::None);
        assert_eq!(f.underline(), Underline::None);
        assert!(f.contains(Flags::ITALIC) && f.contains(Flags::WIDE));
    }

    #[test]
    fn a_complex_cell_resolves_to_its_cluster() {
        let c = Cell::new('x', Color::DEFAULT, Color::DEFAULT, Flags::empty())
            .with_grapheme(GraphemeId(7));
        assert_eq!(c.resolve(), Resolved::Grapheme(GraphemeId(7)));
    }

    #[test]
    fn a_simple_cell_resolves_to_its_character() {
        let c = Cell::new('ß', Color::DEFAULT, Color::DEFAULT, Flags::empty());
        assert_eq!(c.resolve(), Resolved::Char('ß'));
    }

    #[test]
    fn wrap_padding_is_distinguishable_from_a_blank() {
        // Reflow must drop the padding and keep the blank; if they compared
        // equal, unwrapping a line would grow a space out of nothing.
        let pad = Cell::wrap_padding(Color::DEFAULT);
        assert!(pad.is_wrap_padding());
        assert!(!Cell::BLANK.is_wrap_padding());
        assert!(
            !Cell::new(' ', Color::DEFAULT, Color::DEFAULT, Flags::WIDE_SPACER).is_wrap_padding()
        );
    }

    #[test]
    fn a_styled_blank_is_not_trimmable() {
        // Trimming a space with a background colour would erase a highlighted
        // region's tail.
        let template = Cell::BLANK;
        assert!(Cell::BLANK.is_blank_with(template));
        let coloured = Cell::new(' ', Color::DEFAULT, Color::rgb(9, 9, 9), Flags::empty());
        assert!(!coloured.is_blank_with(template));
        let underlined = Cell::new(
            ' ',
            Color::DEFAULT,
            Color::DEFAULT,
            Flags::empty().with_underline(Underline::Single),
        );
        assert!(!underlined.is_blank_with(template));
    }

    #[test]
    fn trimming_is_relative_to_the_current_background() {
        // On a screen painted with a background colour, a cell in that colour
        // *is* blank; against the default it is not.
        let painted = Cell::new(' ', Color::DEFAULT, Color::rgb(9, 9, 9), Flags::empty());
        assert!(painted.is_blank_with(painted));
        assert!(!painted.is_blank_with(Cell::BLANK));
    }
}

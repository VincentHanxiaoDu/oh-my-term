//! Keys in, actions or bytes out.
//!
//! The default is passthrough. omt is a terminal before it is anything else, so
//! a key it has no opinion about reaches the program the user is actually
//! talking to, unchanged — and a handful of keys can never be taken at all,
//! because a terminal where Ctrl+C does not interrupt is a terminal nobody can
//! rescue.

pub mod key;
pub mod keymap;

pub use key::{Chord, ChordParseError, EncodeMode, Key, Modifiers, encode};
pub use keymap::{
    Action, BindingRefused, Keymap, Mode, RESERVED_BINDING, RESERVED_GLOBAL, defaults,
};

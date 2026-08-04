//! Turning what is on screen into something that can be acted on.
//!
//! Recognition is pure: nothing here touches the filesystem or opens anything.
//! It reports what the *text* looks like, and the questions of whether a path
//! exists and whether the user may open it are asked later, separately, by code
//! that is allowed to have effects.

pub mod recognize;

pub use recognize::{ALLOWED_SCHEMES, Match, Target, recognize, scheme_allowed};

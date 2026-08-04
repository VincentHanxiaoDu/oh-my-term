//! Terminal emulation: parser, grid, scrollback, reflow.
//!
//! A pure state machine. Bytes go in; state and damage come out. No I/O, no
//! async, no global state — which is what makes it testable by feeding it
//! bytes and what lets several terminals live in one process without
//! coordinating.

pub mod action;
pub mod cell;
pub mod grid;
pub mod line;
pub mod scrollback;
pub mod terminal;

pub use action::{
    ActionQueue, Backpressure, BlockEvent, ClipboardSelection, ColorSlot, HostAction,
    HyperlinkEvent, TermMode, Warning, WindowOp,
};
pub use cell::{Cell, Color, ColorKind, Flags, GraphemeId, Resolved, Underline};
pub use grid::{Cursor, EraseExtent, Grid, GridSize, Margins};
pub use line::{ExtraAttrs, Generation, HyperlinkId, ImagePlacementId, Line, Wrap, erase_template};
pub use scrollback::{
    Point, Position, Resolution, Scrollback, ScrollbackLimits, unwrap_lines, wrap_line,
    wrapped_rows,
};
pub use terminal::{Modes, ResizeReport, TermConfig, Terminal, Which};

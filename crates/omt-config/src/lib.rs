//! The layered configuration model.
//!
//! Six layers, merged per leaf key, with every resolved value carrying where it
//! came from. The provenance is not a nicety: a setting whose origin cannot be
//! traced is a setting nobody can debug, and "why is my font wrong" is
//! otherwise unanswerable across four files.

pub mod import;
pub mod layer;
pub mod load;
pub mod merge;
pub mod write;

pub use import::{Imported, Source, Unmapped, import, usual_paths};
pub use layer::{APPENDING_ARRAYS, ArrayMerge, Layer, Scope, UNSET};
pub use load::{
    CONFIG_FILE, LoadError, Located, PROJECT_DIR, find_project_config, from_environment, load,
    search_paths,
};
pub use merge::{
    DROPPED_BY_SCOPE, Diagnostic, KeySpec, LayerInput, Provenance, Resolved, Severity, UNKNOWN_KEY,
    merge,
};
pub use write::{WriteError, set_value};

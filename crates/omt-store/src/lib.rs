//! Durable state, and what to do when it comes back damaged.
//!
//! The shape is the standard one — periodic snapshots plus an append-only log —
//! and the value is in the recovery path. A crash mid-write leaves a partial
//! record, which is not corruption but the expected end of a killed process;
//! bytes changed *underneath* a complete record are something else entirely.
//! The two want opposite responses, so the record frame is built to tell them
//! apart, and a restore never silently drops either.

pub mod log;
pub mod record;
pub mod snapshot;

pub use log::{CLEAN_MARKER, Log, RestoreOutcome, restore};
pub use record::{MAGIC, MAX_RECORD, ReadOutcome, crc32, read_record, write_record};
pub use snapshot::{SnapshotError, load_snapshot, write_snapshot};

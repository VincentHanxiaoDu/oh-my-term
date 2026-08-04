//! The omt binary's library half.
//!
//! The capability declarations live here rather than in `main.rs` so the parity
//! gate can enumerate exactly what the binary registers. A gate that checked a
//! separately-maintained list would be checking the wrong thing.

pub mod capabilities;

pub mod run;

pub mod state;

pub mod serve;

//! The omt binary's library half.
//!
//! The capability declarations live here rather than in `main.rs` so the parity
//! gate can enumerate exactly what the binary registers. A gate that checked a
//! separately-maintained list would be checking the wrong thing.

pub mod capabilities;

pub mod attach;
pub mod automation;
pub mod bootstrap;
pub mod cli;
pub mod discovery;
pub mod doctor;
pub mod events;
pub mod media;
pub mod migrate;
pub mod multiuser;
pub mod onboarding;
pub mod ops;
pub mod pairing;
pub mod persist;
pub mod probe;
pub mod queue;
pub mod reach;
pub mod rows;
pub mod run;
pub mod scheduler;
pub mod setup;
pub mod shutdown;
pub mod thin;
pub mod uninstall;
pub mod upgrade;

pub mod state;
pub mod surfaces;
pub mod transcript;

pub mod serve;

pub mod remote;

pub mod web;

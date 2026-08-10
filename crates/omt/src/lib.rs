//! The omt binary's library half.
//!
//! The capability declarations live here rather than in `main.rs` so the parity
//! gate can enumerate exactly what the binary registers. A gate that checked a
//! separately-maintained list would be checking the wrong thing.

pub mod capabilities;

pub mod attach;
pub mod doctor;
pub mod events;
pub mod pairing;
pub mod persist;
pub mod automation;
pub mod bootstrap;
pub mod cli;
pub mod discovery;
pub mod media;
pub mod multiuser;
pub mod upgrade;
pub mod ops;
pub mod queue;
pub mod shutdown;
pub mod migrate;
pub mod onboarding;
pub mod setup;
pub mod thin;
pub mod uninstall;
pub mod reach;
pub mod run;
pub mod scheduler;

pub mod state;
pub mod surfaces;
pub mod transcript;

pub mod serve;

pub mod remote;

pub mod web;

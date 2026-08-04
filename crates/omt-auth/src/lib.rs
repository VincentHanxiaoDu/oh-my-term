//! Credentials, and the role they map to.
//!
//! Three properties here are security bugs the moment they slip: the plain
//! token exists exactly once and is never recoverable from the store; digest
//! comparison is constant-time and does not exit early; and the rejection a
//! caller sees is coarse, because telling somebody their token *existed* but
//! had expired confirms a guess that would otherwise be indistinguishable from
//! noise.

pub mod token;

pub use token::{
    AuthError, Credential, CredentialStore, MintedCredential, RejectionReason, TOKEN_BYTES,
    TOKEN_PREFIX, digest_token, is_well_formed,
};

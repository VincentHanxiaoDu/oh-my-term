//! The capability catalog: declare an operation once, derive every surface.
//!
//! This crate is the door. Every mutation and every read in omt passes through
//! [`CapabilityRegistry::dispatch`], which is what makes surface parity a
//! checkable property rather than a promise — and what makes "remote is
//! equivalent to local" true by construction, since the TUI, the local socket
//! and a remote client differ only in how they reach the same function.
//!
//! Declarations live in the crates that own them and are collected at link
//! time, so this crate never sees them at compile time. That is deliberate:
//! `omt-catalog` must be usable by any crate, including ones that declare
//! event-shaped capabilities, so it can depend on none of them.

mod decl;
mod error;
mod registry;

pub use decl::{Decl, DeclError, DedupKey, Effects, Intent, Kind, Parity, Surface};
pub use error::{CapabilityError, ConflictState, ErrorCode, ErrorDetail};
pub use registry::{
    Capability, CapabilityHandler, CapabilityRegistry, CallContext, DispatchOutcome, RegistryError,
    RequestId,
};

/// The link-time slice every declaring crate appends to.
///
/// A distributed slice rather than life-before-`main` registration: entries are
/// `const` data placed in a section, with no initializer that can be dropped by
/// the linker and no ordering to depend on. The failure mode matters more than
/// the mechanism — a declaration that fails to link is *absent from the dump*
/// and the committed-artifact diff fails, rather than silently vanishing from
/// the catalog.
#[linkme::distributed_slice]
pub static DECLS: [fn() -> &'static Decl];

/// Every declaration the linked binary actually contains, in name order.
///
/// Codegen consumes this — via the binary, not a source scan — so its input is
/// byte-for-byte the list the process registers.
#[must_use]
pub fn linked_decls() -> Vec<&'static Decl> {
    let mut v: Vec<_> = DECLS.iter().map(|f| f()).collect();
    v.sort_unstable_by_key(|d| d.name);
    v
}

/// Declare a capability.
///
/// Expands to the type, its [`Capability`] implementation, its `Decl`, and the
/// [`DECLS`] entry that makes it visible to codegen.
#[macro_export]
macro_rules! capability {
    (
        $(#[$m:meta])*
        $vis:vis struct $name:ident;
        input  = $input:ty,
        output = $output:ty,
        decl   = $decl:expr $(,)?
    ) => {
        $(#[$m])*
        $vis struct $name;

        $crate::paste_decl!($name, $decl);

        impl $crate::Capability for $name {
            type Input = $input;
            type Output = $output;
            const DECL: &'static $crate::Decl = &$name::DECL_STATIC;
        }
    };
}

/// Internal: attaches the static declaration and its link-time entry.
#[doc(hidden)]
#[macro_export]
macro_rules! paste_decl {
    ($name:ident, $decl:expr) => {
        impl $name {
            #[doc(hidden)]
            pub const DECL_STATIC: $crate::Decl = $decl;
        }

        const _: () = {
            fn decl() -> &'static $crate::Decl {
                &$name::DECL_STATIC
            }
            #[linkme::distributed_slice($crate::DECLS)]
            static ENTRY: fn() -> &'static $crate::Decl = decl;
        };
    };
}

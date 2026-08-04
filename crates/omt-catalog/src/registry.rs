//! The capability trait, its handlers, and the registry that dispatches them.

use std::collections::BTreeMap;

use omt_types::{Actor, DeviceId, IntentId, Role};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::decl::{Decl, DeclError, Effects, Kind};
use crate::error::CapabilityError;

/// A declared operation.
///
/// The metadata is `const` and the types are associated, so one declaration
/// yields the schema, the route, the CLI entry and the docs without anything
/// being written twice.
pub trait Capability: Sized + 'static {
    /// What the caller sends.
    type Input: DeserializeOwned + Serialize + schemars::JsonSchema + Send + 'static;
    /// What it gets back.
    type Output: Serialize + schemars::JsonSchema + Send + 'static;

    /// The single source of metadata.
    const DECL: &'static Decl;

    /// Narrow the declared effects for a particular input.
    ///
    /// Lets `pane.split` declare `SPAWNS_PROCESS` and then admit it spawns
    /// nothing when handed an existing session, so the audit log records what
    /// happened rather than what might have.
    ///
    /// Must be pure, and **must not widen**: the result has to be a subset of
    /// `DECL.effects`, which the registry checks rather than trusts.
    #[must_use]
    fn refine_effects(_input: &Self::Input) -> Effects {
        Self::DECL.effects
    }
}

/// Who is calling, and under what constraints.
#[derive(Debug, Clone)]
pub struct CallContext {
    /// The caller.
    pub actor: Actor,
    /// The role their credential maps to.
    pub role: Role,
    /// Stable across reconnection, so a client that lost an acknowledgement can
    /// ask again and find out what happened rather than guessing.
    pub request: RequestId,
    /// Present on mutations, minted by the client at intent time.
    pub intent: Option<IntentId>,
}

/// A request identity that survives a dropped connection.
///
/// `(device, counter)` rather than a per-connection number: the whole point is
/// to be recognisable *after* the connection that issued it is gone.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
pub struct RequestId {
    /// Which device issued it.
    pub device: DeviceId,
    /// Monotonic within that device.
    pub n: u64,
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.device, self.n)
    }
}

/// Implements one capability.
pub trait CapabilityHandler<C: Capability>: Send + Sync + 'static {
    /// Run it.
    ///
    /// # Errors
    /// Whatever the operation can fail with, as a closed-code error.
    fn call(&self, ctx: &CallContext, input: C::Input) -> Result<C::Output, CapabilityError>;
}

/// A handler with its types erased, so the registry can hold them together.
///
/// JSON in, JSON out. The boundary costs a serialization step even for the
/// in-process TUI, and that is the price of there being exactly one dispatch
/// path — which is what makes "remote is equivalent to local" true by
/// construction rather than by discipline.
trait ErasedHandler: Send + Sync {
    fn call_json(
        &self,
        ctx: &CallContext,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError>;

    fn refine_effects_json(&self, input: &serde_json::Value) -> Effects;
}

struct Erased<C: Capability, H: CapabilityHandler<C>> {
    handler: H,
    _marker: std::marker::PhantomData<fn() -> C>,
}

impl<C: Capability, H: CapabilityHandler<C>> ErasedHandler for Erased<C, H> {
    fn call_json(
        &self,
        ctx: &CallContext,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, CapabilityError> {
        let typed: C::Input = serde_json::from_value(input)
            .map_err(|e| CapabilityError::invalid_input(e.to_string()))?;
        let out = self.handler.call(ctx, typed)?;
        serde_json::to_value(out).map_err(|e| CapabilityError::internal(e.to_string()))
    }

    fn refine_effects_json(&self, input: &serde_json::Value) -> Effects {
        serde_json::from_value::<C::Input>(input.clone())
            .map_or(C::DECL.effects, |t| C::refine_effects(&t))
    }
}

/// Everything declared, and everything that implements it.
#[derive(Default)]
pub struct CapabilityRegistry {
    decls: BTreeMap<&'static str, &'static Decl>,
    handlers: BTreeMap<&'static str, Box<dyn ErasedHandler>>,
    sealed: bool,
}

impl std::fmt::Debug for CapabilityRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapabilityRegistry")
            .field("declared", &self.decls.len())
            .field("implemented", &self.handlers.len())
            .field("sealed", &self.sealed)
            .finish()
    }
}

impl CapabilityRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a declaration and its handler.
    ///
    /// # Errors
    /// Refuses a duplicate name or an invalid declaration. Both are startup
    /// failures on purpose: a shadowed capability would behave differently
    /// depending on link order, which is the worst kind of bug to chase.
    pub fn register<C: Capability, H: CapabilityHandler<C>>(
        &mut self,
        handler: H,
    ) -> Result<(), RegistryError> {
        let decl = C::DECL;
        decl.validate()
            .map_err(|errors| RegistryError::InvalidDeclaration {
                name: decl.name,
                errors,
            })?;
        if self.decls.contains_key(decl.name) {
            return Err(RegistryError::Duplicate { name: decl.name });
        }
        self.decls.insert(decl.name, decl);
        self.handlers.insert(
            decl.name,
            Box::new(Erased::<C, H> {
                handler,
                _marker: std::marker::PhantomData,
            }),
        );
        Ok(())
    }

    /// Declare without implementing, for tests that only need metadata.
    ///
    /// # Errors
    /// Refuses a duplicate or invalid declaration.
    pub fn declare_only(&mut self, decl: &'static Decl) -> Result<(), RegistryError> {
        decl.validate()
            .map_err(|errors| RegistryError::InvalidDeclaration {
                name: decl.name,
                errors,
            })?;
        if self.decls.contains_key(decl.name) {
            return Err(RegistryError::Duplicate { name: decl.name });
        }
        self.decls.insert(decl.name, decl);
        Ok(())
    }

    /// Close the registry for registration.
    ///
    /// # Errors
    /// Fails if anything declared has no handler. Strict on purpose: a
    /// "declared but unimplemented" state would be discovered by a user at an
    /// inconvenient hour, whereas this is discovered at boot, by name.
    pub fn seal(&mut self) -> Result<(), RegistryError> {
        let missing: Vec<_> = self
            .decls
            .keys()
            .filter(|n| !self.handlers.contains_key(*n))
            .copied()
            .collect();
        if !missing.is_empty() {
            return Err(RegistryError::MissingHandlers { names: missing });
        }
        self.sealed = true;
        Ok(())
    }

    /// Every declaration, in name order.
    ///
    /// Sorted rather than link-ordered, so generated artifacts are
    /// byte-identical run to run.
    pub fn decls(&self) -> impl Iterator<Item = &'static Decl> + '_ {
        self.decls.values().copied()
    }

    /// One declaration.
    #[must_use]
    pub fn decl(&self, name: &str) -> Option<&'static Decl> {
        self.decls.get(name).copied()
    }

    /// How many are declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.decls.len()
    }

    /// Whether nothing is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }

    /// Invoke a capability by name.
    ///
    /// The single dispatch path. The TUI, the local socket and a remote client
    /// all arrive here, which is why the role check lives here and not in a
    /// transport — a transport that forgot it would be a silent hole.
    ///
    /// # Errors
    /// `not_found` for an unknown name, `unauthorized` below the required role,
    /// or whatever the handler returns.
    pub fn dispatch(
        &self,
        name: &str,
        ctx: &CallContext,
        input: serde_json::Value,
    ) -> Result<DispatchOutcome, CapabilityError> {
        let decl = self
            .decls
            .get(name)
            .copied()
            .ok_or_else(|| CapabilityError::not_found(format!("no capability `{name}`")))?;

        if !ctx.role.satisfies(decl.role) {
            return Err(CapabilityError::unauthorized(format!(
                "`{name}` requires {:?}; caller has {:?}",
                decl.role, ctx.role
            )));
        }

        if decl.kind == Kind::Command && ctx.intent.is_none() {
            return Err(CapabilityError::invalid_input(format!(
                "`{name}` is a command and requires an intent id, so a retry can be recognised"
            )));
        }

        let handler = self.handlers.get(name).ok_or_else(|| {
            CapabilityError::unsupported(format!("`{name}` is declared but not implemented here"))
        })?;

        let effects = handler.refine_effects_json(&input);
        // A refinement that widens would understate nothing and overstate
        // everything — the audit log would record effects the capability never
        // declared. Caught here rather than trusted.
        if !decl.effects.contains(effects) {
            return Err(CapabilityError::internal(format!(
                "`{name}` refined its effects to {effects} which is not a subset of its declared {}",
                decl.effects
            )));
        }

        let output = handler.call_json(ctx, input)?;
        Ok(DispatchOutcome { output, effects })
    }
}

/// What a successful dispatch produced.
#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    /// The handler's output.
    pub output: serde_json::Value,
    /// What it actually did, for the audit log.
    pub effects: Effects,
}

/// What can go wrong building a registry.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// Two declarations claimed one name.
    #[error("`{name}` is declared twice; one would silently shadow the other")]
    Duplicate {
        /// The contested name.
        name: &'static str,
    },
    /// A declaration is malformed.
    #[error("`{name}` is not a valid declaration: {errors:?}")]
    InvalidDeclaration {
        /// The capability.
        name: &'static str,
        /// Everything wrong with it.
        errors: Vec<DeclError>,
    },
    /// Something is declared and not implemented.
    #[error("declared with no handler: {names:?} — found at boot rather than by a caller")]
    MissingHandlers {
        /// The unimplemented capabilities.
        names: Vec<&'static str>,
    },
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    use crate::decl::{Intent, Parity};

    #[derive(Serialize, Deserialize, schemars::JsonSchema)]
    struct EchoIn {
        text: String,
        #[serde(default)]
        spawn: bool,
    }

    #[derive(Serialize, schemars::JsonSchema)]
    struct EchoOut {
        text: String,
    }

    struct Echo;

    static ECHO_DECL: Decl = Decl {
        name: "test.echo",
        group: "test",
        verb: "echo",
        title: "Echo",
        aliases: &[],
        hidden: false,
        hidden_reason: None,
        kind: Kind::Command,
        role: Role::Operator,
        effects: Effects::SPAWNS_PROCESS,
        intent: Some(Intent::Cas),
        parity: Parity::Full,
        since: "0.1.0",
        doc: "Echo the input.",
    };

    impl Capability for Echo {
        type Input = EchoIn;
        type Output = EchoOut;
        const DECL: &'static Decl = &ECHO_DECL;

        fn refine_effects(input: &Self::Input) -> Effects {
            // The conditional-effects case: it only spawns when asked to.
            if input.spawn {
                Effects::SPAWNS_PROCESS
            } else {
                Effects::empty()
            }
        }
    }

    struct EchoHandler;
    impl CapabilityHandler<Echo> for EchoHandler {
        fn call(&self, _ctx: &CallContext, input: EchoIn) -> Result<EchoOut, CapabilityError> {
            Ok(EchoOut { text: input.text })
        }
    }

    fn ctx(role: Role) -> CallContext {
        CallContext {
            actor: Actor::Local,
            role,
            request: RequestId {
                device: DeviceId::new(),
                n: 1,
            },
            intent: Some(IntentId::new()),
        }
    }

    fn registry() -> CapabilityRegistry {
        let mut r = CapabilityRegistry::new();
        r.register::<Echo, _>(EchoHandler).expect("register");
        r
    }

    #[test]
    fn dispatch_runs_the_handler() {
        let r = registry();
        let out = r
            .dispatch(
                "test.echo",
                &ctx(Role::Operator),
                serde_json::json!({"text": "hi"}),
            )
            .expect("dispatch");
        assert_eq!(out.output, serde_json::json!({"text": "hi"}));
    }

    #[test]
    fn a_caller_below_the_role_is_refused_before_the_handler_runs() {
        let r = registry();
        let err = r
            .dispatch(
                "test.echo",
                &ctx(Role::Viewer),
                serde_json::json!({"text": "hi"}),
            )
            .expect_err("must be refused");
        assert_eq!(err.code, crate::error::ErrorCode::Unauthorized);
    }

    #[test]
    fn an_unknown_name_is_not_found() {
        let r = registry();
        let err = r
            .dispatch("test.nope", &ctx(Role::Admin), serde_json::json!({}))
            .expect_err("must be refused");
        assert_eq!(err.code, crate::error::ErrorCode::NotFound);
    }

    #[test]
    fn a_command_without_an_intent_id_is_refused() {
        let r = registry();
        let mut c = ctx(Role::Operator);
        c.intent = None;
        let err = r
            .dispatch("test.echo", &c, serde_json::json!({"text": "hi"}))
            .expect_err("must be refused");
        assert_eq!(err.code, crate::error::ErrorCode::InvalidInput);
    }

    #[test]
    fn effects_are_refined_per_call() {
        let r = registry();
        let quiet = r
            .dispatch(
                "test.echo",
                &ctx(Role::Operator),
                serde_json::json!({"text": "x"}),
            )
            .expect("dispatch");
        assert_eq!(
            quiet.effects,
            Effects::empty(),
            "it did not spawn, so it says so"
        );

        let loud = r
            .dispatch(
                "test.echo",
                &ctx(Role::Operator),
                serde_json::json!({"text": "x", "spawn": true}),
            )
            .expect("dispatch");
        assert_eq!(loud.effects, Effects::SPAWNS_PROCESS);
    }

    #[test]
    fn malformed_input_is_invalid_not_internal() {
        let r = registry();
        let err = r
            .dispatch(
                "test.echo",
                &ctx(Role::Operator),
                serde_json::json!({"wrong": 1}),
            )
            .expect_err("must be refused");
        assert_eq!(err.code, crate::error::ErrorCode::InvalidInput);
    }

    #[test]
    fn duplicate_registration_is_refused() {
        let mut r = registry();
        let err = r
            .register::<Echo, _>(EchoHandler)
            .expect_err("duplicate must be refused");
        assert_eq!(err, RegistryError::Duplicate { name: "test.echo" });
    }

    #[test]
    fn seal_fails_naming_an_unimplemented_capability() {
        let mut r = CapabilityRegistry::new();
        r.declare_only(&ECHO_DECL).expect("declare");
        let err = r.seal().expect_err("must refuse to seal");
        assert_eq!(
            err,
            RegistryError::MissingHandlers {
                names: vec!["test.echo"]
            }
        );
    }

    #[test]
    fn seal_succeeds_when_everything_is_implemented() {
        let mut r = registry();
        r.seal().expect("seal");
    }

    #[test]
    fn declarations_iterate_in_name_order() {
        // Determinism of generated artifacts rests on this.
        let r = registry();
        let names: Vec<_> = r.decls().map(|d| d.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }
}

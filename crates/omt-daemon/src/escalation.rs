//! What authority a plugin acts with.
//!
//! `docs/architecture/11-plugins.md` §4.2, and the one rule everything else
//! there is machinery for:
//!
//! > **A plugin may never act with more authority than the actor that triggered
//! > it.**
//!
//! This lives in the daemon's dispatch rather than in `omt-plugin-host`, so a
//! bug in the host cannot bypass it. The host decides what a plugin *asked*
//! for; only dispatch decides what a call actually carries, and the two are
//! deliberately not the same code.
//!
//! The confused-deputy case is the one to keep in mind. A read-only invite link
//! hands somebody a `Viewer`. If a plugin granted `Operator` could act at its
//! own level whenever a `Viewer` poked it, the invite link would have become a
//! way to type into the terminal — through code the user installed for
//! something else entirely.

use omt_types::{Actor, Role};

/// The role a plugin-originated call carries.
///
/// The minimum of what the plugin was granted and what the actor that triggered
/// it holds. With no triggering actor — a plugin acting on an event — the
/// ceiling is `Operator`: `Admin` is reachable only when an `Admin` explicitly
/// invokes a plugin capability, never by an event arriving.
#[must_use]
pub fn effective_role(granted: Role, trigger: Option<Role>) -> Role {
    let ceiling = trigger.unwrap_or(Role::Operator);
    if granted < ceiling { granted } else { ceiling }
}

/// The actor a plugin-originated call carries.
#[must_use]
pub fn effective_actor(plugin: &str, trigger: Option<Actor>) -> Actor {
    Actor::Plugin {
        plugin: plugin.to_owned(),
        // `Local` stands for "omt itself asked", which is what an event is.
        // Recorded rather than elided so the audit log can say a plugin acted
        // on its own initiative.
        on_behalf_of: Box::new(trigger.unwrap_or(Actor::Local)),
    }
}

/// Whether a call at `carried` may invoke a capability that requires `required`.
///
/// Checked before the plugin is contacted, so a plugin cannot be used as a
/// confused deputy by a read-only invite link.
#[must_use]
pub fn permits(carried: Role, required: Role) -> bool {
    carried >= required
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_never_exceeds_the_actor_that_triggered_it() {
        assert_eq!(
            effective_role(Role::Admin, Some(Role::Viewer)),
            Role::Viewer
        );
        assert_eq!(
            effective_role(Role::Admin, Some(Role::Operator)),
            Role::Operator
        );
    }

    #[test]
    fn a_plugin_never_exceeds_what_it_was_granted_either() {
        // The minimum of the two, not the trigger alone: an Admin poking a
        // plugin granted Viewer does not lend it their authority.
        assert_eq!(
            effective_role(Role::Viewer, Some(Role::Admin)),
            Role::Viewer
        );
    }

    #[test]
    fn an_event_can_never_reach_admin() {
        // Admin is reachable only when an Admin explicitly invokes a plugin
        // capability. Otherwise any event on the bus is an Admin trigger.
        assert_eq!(effective_role(Role::Admin, None), Role::Operator);
    }

    #[test]
    fn an_event_still_respects_a_narrower_grant() {
        assert_eq!(effective_role(Role::Viewer, None), Role::Viewer);
    }

    #[test]
    fn an_operator_capability_poked_by_a_viewer_is_refused_before_the_plugin_is_contacted() {
        // The confused-deputy case: otherwise a read-only invite link becomes a
        // way to type into the terminal, through code installed for something
        // else.
        let carried = effective_role(Role::Operator, Some(Role::Viewer));
        assert!(!permits(carried, Role::Operator));
        assert!(permits(carried, Role::Viewer));
    }

    #[test]
    fn an_admin_invoking_a_plugin_granted_admin_gets_admin() {
        // The rule is a ceiling, not a demotion.
        assert_eq!(effective_role(Role::Admin, Some(Role::Admin)), Role::Admin);
    }

    #[test]
    fn the_audit_record_names_both_actors() {
        // So `interaction.resolve` performed by a plugin on behalf of a phone
        // shows as such in every surface.
        let device = omt_types::DeviceId::new();
        let actor = effective_actor(
            "aider-adapter",
            Some(Actor::Remote {
                identity: omt_types::IdentityId::new(),
                device,
            }),
        );
        let rendered = actor.to_string();
        assert!(rendered.contains("aider-adapter"), "{rendered}");
        assert!(
            rendered.contains(&device.to_string()),
            "the device it acted for is named too: {rendered}"
        );
    }

    #[test]
    fn a_plugin_acting_on_its_own_initiative_says_so() {
        // Elided provenance would read as if somebody had asked.
        let actor = effective_actor("aider-adapter", None);
        let Actor::Plugin { on_behalf_of, .. } = actor else {
            panic!("a plugin actor");
        };
        assert_eq!(*on_behalf_of, Actor::Local);
    }

    #[test]
    fn every_role_pairing_is_covered_and_none_of_them_escalates() {
        // A table rather than a spot check: the rule is about the whole matrix,
        // and an ordering bug shows up in one cell.
        for granted in [Role::Viewer, Role::Operator, Role::Admin] {
            for trigger in [
                None,
                Some(Role::Viewer),
                Some(Role::Operator),
                Some(Role::Admin),
            ] {
                let carried = effective_role(granted, trigger);
                assert!(carried <= granted, "{granted:?} {trigger:?}");
                if let Some(trigger) = trigger {
                    assert!(carried <= trigger, "{granted:?} {trigger:?}");
                } else {
                    assert!(carried <= Role::Operator, "{granted:?} via an event");
                }
            }
        }
    }
}

//! Identities and the devices that belong to them.
//!
//! The shape that matters: a device is registered against an identity, and
//! revocation is per *device*. Losing a phone must not mean rotating every
//! credential the person has on every machine — that is the kind of recovery
//! nobody performs, so they carry on with the lost phone still authorized.

use std::collections::BTreeMap;

use omt_types::{DeviceId, IdentityId, Timestamp};
use serde::{Deserialize, Serialize};

/// One registered device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Its identity.
    pub id: DeviceId,
    /// Whose it is.
    pub identity: IdentityId,
    /// What the user calls it.
    pub name: String,
    /// Its public key, which never leaves the device.
    pub public_key: String,
    /// When it was registered.
    pub registered_at: Timestamp,
    /// When it was last seen.
    pub last_seen: Option<Timestamp>,
    /// Whether it has been revoked.
    pub revoked: bool,
}

/// A person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Its identity.
    pub id: IdentityId,
    /// Their display name.
    pub name: String,
    /// When they were added.
    pub created_at: Timestamp,
}

/// Why a registration or lookup failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    /// No such identity.
    #[error("no identity {0}")]
    NoIdentity(IdentityId),
    /// A device with that key is already registered.
    #[error("that device key is already registered as `{name}`")]
    DuplicateKey {
        /// What it is already called.
        name: String,
    },
    /// The key is not usable.
    #[error("a device key cannot be empty")]
    EmptyKey,
}

/// Every identity and device an instance knows.
#[derive(Debug, Default)]
pub struct Directory {
    identities: BTreeMap<IdentityId, Identity>,
    devices: BTreeMap<DeviceId, Device>,
}

impl Directory {
    /// An empty directory.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a person.
    pub fn add_identity(&mut self, name: &str) -> IdentityId {
        let id = IdentityId::new();
        self.identities.insert(
            id,
            Identity {
                id,
                name: name.to_owned(),
                created_at: Timestamp::now(),
            },
        );
        id
    }

    /// Register a device to an identity.
    ///
    /// # Errors
    /// Fails if the identity is unknown, the key is empty, or the key is
    /// already registered — the last because two devices sharing a key makes
    /// revoking one revoke both, and the user would have no way to tell.
    pub fn register_device(
        &mut self,
        identity: IdentityId,
        name: &str,
        public_key: &str,
    ) -> Result<DeviceId, IdentityError> {
        if !self.identities.contains_key(&identity) {
            return Err(IdentityError::NoIdentity(identity));
        }
        if public_key.trim().is_empty() {
            return Err(IdentityError::EmptyKey);
        }
        if let Some(existing) = self.devices.values().find(|d| d.public_key == public_key) {
            return Err(IdentityError::DuplicateKey {
                name: existing.name.clone(),
            });
        }
        let id = DeviceId::new();
        self.devices.insert(
            id,
            Device {
                id,
                identity,
                name: name.to_owned(),
                public_key: public_key.to_owned(),
                registered_at: Timestamp::now(),
                last_seen: None,
                revoked: false,
            },
        );
        Ok(id)
    }

    /// A device.
    #[must_use]
    pub fn device(&self, id: DeviceId) -> Option<&Device> {
        self.devices.get(&id)
    }

    /// An identity.
    #[must_use]
    pub fn identity(&self, id: IdentityId) -> Option<&Identity> {
        self.identities.get(&id)
    }

    /// Every device belonging to an identity, in a stable order.
    #[must_use]
    pub fn devices_of(&self, identity: IdentityId) -> Vec<&Device> {
        self.devices
            .values()
            .filter(|d| d.identity == identity)
            .collect()
    }

    /// Revoke one device.
    ///
    /// One device, never the identity. Losing a phone must not mean re-pairing
    /// a laptop — a recovery that costs too much is one nobody performs, and
    /// then the lost phone stays authorized.
    pub fn revoke_device(&mut self, id: DeviceId) -> bool {
        if let Some(d) = self.devices.get_mut(&id) {
            d.revoked = true;
            return true;
        }
        false
    }

    /// Whether a device may currently be used.
    #[must_use]
    pub fn is_active(&self, id: DeviceId) -> bool {
        self.devices.get(&id).is_some_and(|d| !d.revoked)
    }

    /// Record that a device was seen.
    pub fn touch(&mut self, id: DeviceId, at: Timestamp) {
        if let Some(d) = self.devices.get_mut(&id) {
            d.last_seen = Some(at);
        }
    }

    /// Every identity this instance knows.
    ///
    /// Absent, nothing could ask "who is there" — only "tell me about this one",
    /// which needs an id somebody already has. That is a large part of why this
    /// directory had no callers: it could answer questions nobody was in a
    /// position to ask.
    #[must_use]
    pub fn identities(&self) -> Vec<&Identity> {
        self.identities.values().collect()
    }

    /// Every device, whoever it belongs to.
    #[must_use]
    pub fn devices(&self) -> Vec<&Device> {
        self.devices.values().collect()
    }

    /// Look a device up by the key it presented.
    #[must_use]
    pub fn by_key(&self, public_key: &str) -> Option<&Device> {
        self.devices.values().find(|d| d.public_key == public_key)
    }
}

/// Why an instance refused a device grant.
///
/// `docs/architecture/23-identity-and-devices.md` §3.3. Each carries the exact
/// fix, because a generic auth error here sends somebody to check a passphrase
/// that is fine — the usual cause is that this machine has simply never been
/// told about this identity, which is a one-command problem they will never
/// guess at from "unauthorized".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum GrantRefusal {
    /// This instance has never enrolled the identity.
    IdentityNotEnrolled {
        /// What to do, naming this instance.
        fix: String,
    },
    /// The grant was signed by a root this instance does not know.
    UnknownRoot,
    /// The grant has expired.
    Expired {
        /// When it did, seconds since the epoch.
        at: u64,
    },
    /// The grant was revoked.
    Revoked,
    /// The grant predates this instance's epoch floor.
    ///
    /// Distinct from revoked: a floor invalidates every grant issued before a
    /// moment, which is what somebody does after losing a laptop and not
    /// knowing which grants were on it.
    BelowEpochFloor {
        /// The floor.
        floor: u64,
    },
}

/// What an instance knows when checking a grant.
#[derive(Debug, Clone)]
pub struct Enrolment {
    /// This instance's name, for the message.
    pub instance: String,
    /// Identities this instance has enrolled.
    pub enrolled: Vec<IdentityId>,
    /// Root keys it will accept a grant from.
    pub roots: Vec<String>,
    /// Grants that have been revoked, by id.
    pub revoked: Vec<String>,
    /// The oldest grant epoch still accepted.
    pub epoch_floor: u64,
}

/// A device grant, as presented.
#[derive(Debug, Clone)]
pub struct Grant {
    /// Its id.
    pub id: String,
    /// Whose it is.
    pub identity: IdentityId,
    /// Which root signed it.
    pub root: String,
    /// When it stops being valid.
    pub not_after: u64,
    /// When it was issued.
    pub epoch: u64,
}

/// Check a grant against what this instance knows.
///
/// Checks the enrolment first. It is the only refusal with a fix the person can
/// act on, and finding out about a bad signature when the real problem is that
/// this machine has never heard of you is a wasted afternoon.
#[must_use]
pub fn check_grant(grant: &Grant, at: u64, enrolment: &Enrolment) -> Option<GrantRefusal> {
    if !enrolment.enrolled.contains(&grant.identity) {
        return Some(GrantRefusal::IdentityNotEnrolled {
            fix: format!(
                "`{}` doesn't know this identity yet. Run `omt identity enroll` on {}, or open an invite from it.",
                enrolment.instance, enrolment.instance
            ),
        });
    }
    if !enrolment.roots.contains(&grant.root) {
        return Some(GrantRefusal::UnknownRoot);
    }
    if enrolment.revoked.contains(&grant.id) {
        return Some(GrantRefusal::Revoked);
    }
    if grant.epoch < enrolment.epoch_floor {
        return Some(GrantRefusal::BelowEpochFloor {
            floor: enrolment.epoch_floor,
        });
    }
    if grant.not_after <= at {
        return Some(GrantRefusal::Expired {
            at: grant.not_after,
        });
    }
    None
}

/// Whether taking the `home` role changes how an instance runs sessions.
///
/// It does not. Written as a function because the tempting implementation
/// makes home special in some other way — routing through it, or trusting it
/// more — and then losing the home machine takes the others with it.
#[must_use]
pub const fn home_changes_session_behaviour() -> bool {
    false
}

/// What kind of thing a device is, which drives the icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// A laptop or desktop.
    Computer,
    /// A phone.
    Phone,
    /// A tablet.
    Tablet,
    /// Something omt has no icon for.
    ///
    /// Present so an unknown device renders as a device rather than as
    /// nothing. A missing icon on an attribution reads as a bug in the
    /// attribution, which is the one thing it must not.
    Unknown,
}

/// A device, as every surface renders it.
///
/// `docs/architecture/23-identity-and-devices.md` §8.1. Denormalised onto every
/// presence, actor and resolution, so no surface has to look anything up to
/// draw an attribution.
///
/// Denormalised on purpose. A client that had to resolve an id would show
/// "dev_5TR9K2QF answered this" until the lookup came back — and on the surface
/// that matters most, a phone that has just woken up, the lookup is exactly what
/// has not happened yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceRef {
    /// Its id.
    pub id: DeviceId,
    /// What its owner calls it.
    pub label: String,
    /// What kind of thing it is.
    pub form: Form,
    /// A short platform line, for a subtitle.
    pub platform_short: String,
    /// Whether this is the device being rendered on.
    ///
    /// Computed per recipient, because it is the one field whose answer
    /// differs for every reader — and a client that worked it out itself would
    /// need its own device id in a place it does not otherwise need it.
    pub is_this_device: bool,
}

/// Build the reference a particular recipient should see.
#[must_use]
pub fn device_ref(device: &Device, form: Form, platform: &str, recipient: DeviceId) -> DeviceRef {
    DeviceRef {
        id: device.id,
        // Never empty. An attribution reading "answered by" with nothing after
        // it is worse than no attribution: it looks like the answer came from
        // somewhere omt could not name.
        label: if device.name.trim().is_empty() {
            "an unnamed device".to_owned()
        } else {
            device.name.clone()
        },
        form,
        platform_short: platform.to_owned(),
        is_this_device: device.id == recipient,
    }
}

/// How an attribution reads.
///
/// "You" for the reader's own device, so somebody who answered on their phone
/// and is now looking at their laptop sees that they did it — the common case
/// for this feature is one person with several devices, not several people.
#[must_use]
pub fn attribution(reference: &DeviceRef) -> String {
    if reference.is_this_device {
        "you, on this device".to_owned()
    } else {
        format!("you, on {}", reference.label)
    }
}

/// How an attribution reads when the actor is somebody else.
#[must_use]
pub fn attribution_for_other(who: &str, reference: &DeviceRef) -> String {
    format!("{who}, on {}", reference.label)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;
    fn a_device(name: &str) -> Device {
        let mut d = Directory::new();
        let me = d.add_identity("me");
        let id = d.register_device(me, name, "k-1").expect("register");
        d.device(id).expect("present").clone()
    }

    #[test]
    fn a_reference_carries_everything_needed_to_draw_it() {
        // A client that had to resolve an id would show "dev_5TR9K2QF answered
        // this" until the lookup came back — and on a phone that has just woken
        // up, the lookup is exactly what has not happened yet.
        let device = a_device("iPhone");
        let reference = device_ref(&device, Form::Phone, "iOS · Safari", DeviceId::new());
        assert_eq!(reference.label, "iPhone");
        assert_eq!(reference.form, Form::Phone);
        assert_eq!(reference.platform_short, "iOS · Safari");
    }

    #[test]
    fn whether_it_is_this_device_is_computed_per_recipient() {
        // The one field whose answer differs for every reader.
        let device = a_device("iPhone");
        let here = device_ref(&device, Form::Phone, "iOS", device.id);
        let there = device_ref(&device, Form::Phone, "iOS", DeviceId::new());
        assert!(here.is_this_device);
        assert!(!there.is_this_device);
    }

    #[test]
    fn an_unnamed_device_still_reads_as_a_device() {
        // An attribution reading "answered by" with nothing after it looks
        // like the answer came from somewhere omt could not name.
        let mut device = a_device("x");
        device.name = "   ".to_owned();
        let reference = device_ref(&device, Form::Unknown, "", DeviceId::new());
        assert_eq!(reference.label, "an unnamed device");
    }

    #[test]
    fn an_unknown_form_is_a_form_rather_than_an_absence() {
        // A missing icon on an attribution reads as a bug in the attribution,
        // which is the one thing it must not.
        let device = a_device("something");
        let reference = device_ref(&device, Form::Unknown, "", DeviceId::new());
        assert_eq!(reference.form, Form::Unknown);
    }

    #[test]
    fn your_own_answer_from_another_device_says_it_was_you() {
        // The common case for this feature is one person with several devices,
        // not several people.
        let device = a_device("iPhone");
        let reference = device_ref(&device, Form::Phone, "iOS", DeviceId::new());
        assert_eq!(attribution(&reference), "you, on iPhone");
    }

    #[test]
    fn your_answer_on_the_device_you_are_reading_says_so() {
        let device = a_device("MacBook");
        let reference = device_ref(&device, Form::Computer, "macOS", device.id);
        assert_eq!(attribution(&reference), "you, on this device");
    }

    #[test]
    fn somebody_elses_answer_names_them_and_their_device() {
        let device = a_device("their laptop");
        let reference = device_ref(&device, Form::Computer, "Linux", DeviceId::new());
        assert_eq!(
            attribution_for_other("Kenji", &reference),
            "Kenji, on their laptop"
        );
    }

    #[test]
    fn a_reference_serialises_with_every_field_a_client_renders() {
        let device = a_device("iPhone");
        let reference = device_ref(&device, Form::Phone, "iOS · Safari", DeviceId::new());
        let json = serde_json::to_value(&reference).expect("json");
        for field in ["id", "label", "form", "platform_short", "is_this_device"] {
            assert!(json.get(field).is_some(), "`{field}` is missing");
        }
    }

    fn enrolment(identity: IdentityId) -> Enrolment {
        Enrolment {
            instance: "mini".to_owned(),
            enrolled: vec![identity],
            roots: vec!["root-1".to_owned()],
            revoked: Vec::new(),
            epoch_floor: 100,
        }
    }

    fn grant(identity: IdentityId) -> Grant {
        Grant {
            id: "grant-1".to_owned(),
            identity,
            root: "root-1".to_owned(),
            not_after: 2_000,
            epoch: 500,
        }
    }

    #[test]
    fn a_good_grant_is_accepted() {
        let id = IdentityId::new();
        assert_eq!(check_grant(&grant(id), 1_000, &enrolment(id)), None);
    }

    #[test]
    fn an_unenrolled_identity_is_told_exactly_what_to_run() {
        // A generic auth error sends somebody to check a passphrase that is
        // fine. The usual cause is that this machine has never been told about
        // this identity, which they will never guess from "unauthorized".
        let mut e = enrolment(IdentityId::new());
        e.enrolled.clear();
        let refusal = check_grant(&grant(IdentityId::new()), 1_000, &e).expect("refused");
        let GrantRefusal::IdentityNotEnrolled { fix } = refusal else {
            panic!("wrong refusal");
        };
        assert!(fix.contains("omt identity enroll"), "{fix}");
        assert!(fix.contains("mini"), "{fix}");
    }

    #[test]
    fn enrolment_is_checked_before_the_signature() {
        // Finding out about a bad root when the real problem is that this
        // machine has never heard of you is a wasted afternoon.
        let id = IdentityId::new();
        let mut e = enrolment(id);
        e.enrolled.clear();
        e.roots.clear();
        assert!(matches!(
            check_grant(&grant(id), 1_000, &e),
            Some(GrantRefusal::IdentityNotEnrolled { .. })
        ));
    }

    #[test]
    fn a_grant_from_an_unknown_root_is_refused() {
        let id = IdentityId::new();
        let mut g = grant(id);
        g.root = "somebody-elses-root".to_owned();
        assert_eq!(
            check_grant(&g, 1_000, &enrolment(id)),
            Some(GrantRefusal::UnknownRoot)
        );
    }

    #[test]
    fn a_revoked_grant_is_refused() {
        let id = IdentityId::new();
        let mut e = enrolment(id);
        e.revoked.push("grant-1".to_owned());
        assert_eq!(
            check_grant(&grant(id), 1_000, &e),
            Some(GrantRefusal::Revoked)
        );
    }

    #[test]
    fn an_expired_grant_says_when_it_expired() {
        // So somebody can tell "my grant is old" from "my clock is wrong",
        // which look identical without the date.
        let id = IdentityId::new();
        let refusal = check_grant(&grant(id), 9_000, &enrolment(id)).expect("refused");
        assert_eq!(refusal, GrantRefusal::Expired { at: 2_000 });
    }

    #[test]
    fn an_epoch_floor_invalidates_everything_older_at_once() {
        // Distinct from revocation: it is what somebody does after losing a
        // laptop and not knowing which grants were on it.
        let id = IdentityId::new();
        let mut old = grant(id);
        old.epoch = 50;
        assert_eq!(
            check_grant(&old, 1_000, &enrolment(id)),
            Some(GrantRefusal::BelowEpochFloor { floor: 100 })
        );
    }

    #[test]
    fn the_home_role_does_not_change_how_sessions_run() {
        // The tempting implementation makes home special in some other way —
        // routing through it, or trusting it more — and then losing the home
        // machine takes the others with it.
        assert!(!home_changes_session_behaviour());
    }

    #[test]
    fn every_refusal_is_distinguishable_on_the_wire() {
        // A client that cannot tell them apart shows one message for five
        // different problems, four of which it is wrong about.
        let mut seen = std::collections::BTreeSet::new();
        for refusal in [
            GrantRefusal::IdentityNotEnrolled {
                fix: "x".to_owned(),
            },
            GrantRefusal::UnknownRoot,
            GrantRefusal::Expired { at: 1 },
            GrantRefusal::Revoked,
            GrantRefusal::BelowEpochFloor { floor: 1 },
        ] {
            let json = serde_json::to_value(&refusal).expect("json");
            let code = json
                .get("code")
                .and_then(|c| c.as_str())
                .expect("a code")
                .to_owned();
            assert!(seen.insert(code));
        }
        assert_eq!(seen.len(), 5);
    }

    fn directory() -> (Directory, IdentityId) {
        let mut d = Directory::new();
        let me = d.add_identity("me");
        (d, me)
    }

    #[test]
    fn a_device_registers_against_an_identity() {
        let (mut d, me) = directory();
        let phone = d
            .register_device(me, "phone", "ed25519:aaa")
            .expect("register");
        assert_eq!(d.device(phone).expect("present").identity, me);
        assert!(d.is_active(phone));
    }

    #[test]
    fn one_identity_holds_several_devices() {
        let (mut d, me) = directory();
        d.register_device(me, "phone", "k1").expect("phone");
        d.register_device(me, "laptop", "k2").expect("laptop");
        assert_eq!(d.devices_of(me).len(), 2);
    }

    #[test]
    fn revoking_a_phone_leaves_the_laptop_alone() {
        // The whole reason revocation is per device: a recovery that costs too
        // much is one nobody performs, and then the lost phone stays
        // authorized.
        let (mut d, me) = directory();
        let phone = d.register_device(me, "phone", "k1").expect("phone");
        let laptop = d.register_device(me, "laptop", "k2").expect("laptop");
        assert!(d.revoke_device(phone));
        assert!(!d.is_active(phone));
        assert!(d.is_active(laptop), "the laptop was not re-paired");
    }

    #[test]
    fn a_revoked_device_is_kept_so_its_reuse_is_visible() {
        let (mut d, me) = directory();
        let phone = d.register_device(me, "phone", "k1").expect("phone");
        d.revoke_device(phone);
        assert!(
            d.device(phone).is_some_and(|dev| dev.revoked),
            "deleting it would make a stolen device's attempts look like noise"
        );
    }

    #[test]
    fn two_devices_cannot_share_a_key() {
        // Sharing one makes revoking either revoke both, and the user has no
        // way to tell that happened.
        let (mut d, me) = directory();
        d.register_device(me, "phone", "same").expect("first");
        let err = d
            .register_device(me, "tablet", "same")
            .expect_err("must refuse");
        assert!(
            matches!(err, IdentityError::DuplicateKey { ref name } if name == "phone"),
            "{err:?}"
        );
    }

    #[test]
    fn a_device_cannot_register_to_an_identity_that_does_not_exist() {
        let mut d = Directory::new();
        assert!(matches!(
            d.register_device(IdentityId::new(), "ghost", "k"),
            Err(IdentityError::NoIdentity(_))
        ));
    }

    #[test]
    fn an_empty_key_is_refused() {
        // A device that registered with no key would be indistinguishable from
        // every other device that did the same.
        let (mut d, me) = directory();
        assert_eq!(
            d.register_device(me, "phone", "   "),
            Err(IdentityError::EmptyKey)
        );
    }

    #[test]
    fn a_device_is_findable_by_the_key_it_presents() {
        let (mut d, me) = directory();
        let phone = d
            .register_device(me, "phone", "ed25519:xyz")
            .expect("register");
        assert_eq!(d.by_key("ed25519:xyz").map(|x| x.id), Some(phone));
        assert!(d.by_key("ed25519:nope").is_none());
    }

    #[test]
    fn last_seen_starts_unset_and_is_recorded() {
        // Unset is honest: a device registered from a QR code has genuinely
        // never connected, and showing its registration time as a visit would
        // be a small lie the user would act on.
        let (mut d, me) = directory();
        let phone = d.register_device(me, "phone", "k").expect("register");
        assert!(d.device(phone).expect("present").last_seen.is_none());
        let at = Timestamp::from_unix_seconds(1_000);
        d.touch(phone, at);
        assert_eq!(d.device(phone).expect("present").last_seen, Some(at));
    }

    #[test]
    fn an_unknown_device_is_not_active() {
        let d = Directory::new();
        assert!(!d.is_active(DeviceId::new()));
    }

    #[test]
    fn revoking_something_that_does_not_exist_says_so() {
        let mut d = Directory::new();
        assert!(!d.revoke_device(DeviceId::new()));
    }
}

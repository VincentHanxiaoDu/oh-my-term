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

    /// Look a device up by the key it presented.
    #[must_use]
    pub fn by_key(&self, public_key: &str) -> Option<&Device> {
        self.devices.values().find(|d| d.public_key == public_key)
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

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

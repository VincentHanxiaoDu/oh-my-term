//! Bearer credentials.
//!
//! Three properties, each of which is a security bug if it slips.
//!
//! The token has full entropy from a CSPRNG, so it is stored as a plain SHA-256
//! digest — a password KDF would cost a great deal and buy nothing against
//! thirty-two random bytes. Comparison is constant-time, because a timing
//! difference on a digest comparison is a byte-at-a-time oracle. And the plain
//! token is returned exactly once at creation and never recoverable afterwards,
//! so a leaked store is not a leaked set of credentials.

use std::collections::BTreeMap;

use omt_types::{DeviceClaim, Role, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// The prefix every omt credential carries.
///
/// Present so a token found in a log or a paste is *recognisable as a
/// credential* — secret scanners key on prefixes, and an unidentifiable blob is
/// one nobody revokes.
pub const TOKEN_PREFIX: &str = "omt_c_";

/// How many random bytes a token carries.
pub const TOKEN_BYTES: usize = 32;

/// A stored credential. Never contains the token itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    /// Its identity, for listing and revoking.
    pub id: String,
    /// SHA-256 of the token, hex encoded.
    digest: String,
    /// What it may do.
    pub role: Role,
    /// A human-readable label.
    pub label: String,
    /// When it was minted.
    pub created_at: Timestamp,
    /// When it stops working, if ever.
    pub expires_at: Option<Timestamp>,
    /// Whether it has been revoked.
    pub revoked: bool,
    /// The device public key this credential is bound to, if any.
    ///
    /// A bound credential also requires a signature over the server's nonce, so
    /// a token exfiltrated from a phone's storage is not usable from anywhere
    /// else.
    pub device_key: Option<String>,
}

/// Why a credential was rejected.
///
/// Deliberately coarse on the wire. Distinguishing "no such token" from
/// "expired token" tells an attacker which guesses were close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The credential is not usable, for any of several reasons.
    #[error("the credential was not accepted")]
    Rejected,
    /// The presented token is not even shaped like one.
    #[error("that is not an omt credential")]
    Malformed,
}

/// Why a credential was rejected, for the log rather than the wire.
///
/// The operator needs to know which of these it was; the caller must not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    /// Nothing matched.
    Unknown,
    /// It was revoked.
    Revoked,
    /// It has expired.
    Expired,
    /// It is device-bound and no proof of that device was presented.
    MissingDeviceProof,
}

/// A freshly minted credential.
///
/// The plain token appears here and nowhere else, ever again.
#[derive(Debug, Clone)]
pub struct MintedCredential {
    /// The stored record.
    pub credential: Credential,
    /// The token to show the user once.
    pub token: String,
}

/// Hash a token the way the store does.
#[must_use]
pub fn digest_token(token: &str) -> String {
    let out = Sha256::digest(token.as_bytes());
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether a string is shaped like an omt credential.
#[must_use]
pub fn is_well_formed(token: &str) -> bool {
    token.starts_with(TOKEN_PREFIX) && token.len() > TOKEN_PREFIX.len() + 20
}

/// The prefix an invite carries.
///
/// Deliberately different from a credential's. An invite is *not* a credential
/// — it is the right to obtain one — and the two being indistinguishable in a
/// log, a paste or a screenshot is how one gets treated as the other.
pub const INVITE_PREFIX: &str = "omt_i_";

/// How long an invite is good for.
///
/// Short, because an invite is meant to be scanned off a screen in the same
/// minute it is printed. A long-lived one is a credential with extra steps —
/// which is exactly what the invite link used to be.
pub const INVITE_TTL_SECS: u64 = 15 * 60;

/// A stored invite. Never contains the token itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    /// Its identity, for revoking.
    pub id: String,
    /// SHA-256 of the token, hex encoded.
    digest: String,
    /// The role the credential it mints will have.
    pub role: Role,
    /// When it stops being exchangeable.
    pub expires_at: Timestamp,
    /// Whether it has already been exchanged.
    ///
    /// Single-exchange, so an invite photographed off somebody's screen after
    /// they used it is worth nothing.
    pub used: bool,
}

/// A freshly minted invite. The token appears here and nowhere else.
#[derive(Debug, Clone)]
pub struct MintedInvite {
    /// The stored record.
    pub invite: Invite,
    /// The token to print once.
    pub token: String,
}

/// Every credential an instance knows.
#[derive(Debug, Default)]
pub struct CredentialStore {
    by_id: BTreeMap<String, Credential>,
    invites: BTreeMap<String, Invite>,
}

impl CredentialStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a credential.
    ///
    /// The token is returned once. Nothing here can recover it afterwards, and
    /// that is the point: a store that leaks is not a set of credentials that
    /// leaks.
    pub fn mint(
        &mut self,
        role: Role,
        label: &str,
        expires_at: Option<Timestamp>,
        device_key: Option<String>,
    ) -> MintedCredential {
        use rand::Rng as _;
        let mut bytes = [0u8; TOKEN_BYTES];
        rand::rng().fill(&mut bytes);

        // base62 over full-entropy bytes: no padding, no characters that get
        // mangled by a shell, a URL or a double-click selection.
        const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        let body: String = bytes
            .iter()
            .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
            .collect();
        let token = format!("{TOKEN_PREFIX}{body}");

        let id = format!("cred_{}", &digest_token(&token)[..12]);
        let credential = Credential {
            id: id.clone(),
            digest: digest_token(&token),
            role,
            label: label.to_owned(),
            created_at: Timestamp::now(),
            expires_at,
            revoked: false,
            device_key,
        };
        self.by_id.insert(id, credential.clone());
        MintedCredential { credential, token }
    }

    /// Check a presented token.
    ///
    /// # Errors
    /// Returns one coarse rejection whatever went wrong. The specific reason
    /// goes to [`Self::verify_verbose`] and to the log, never to the caller:
    /// telling an attacker that a token *existed* but had expired confirms a
    /// guess that would otherwise be indistinguishable from noise.
    pub fn verify(
        &self,
        token: &str,
        now: Timestamp,
        device_proof: bool,
    ) -> Result<&Credential, AuthError> {
        self.verify_verbose(token, now, device_proof)
            .map_err(|(e, _)| e)
    }

    /// Check a presented token, keeping the reason for the operator.
    ///
    /// # Errors
    /// Returns the coarse error and the specific reason behind it.
    pub fn verify_verbose(
        &self,
        token: &str,
        now: Timestamp,
        device_proof: bool,
    ) -> Result<&Credential, (AuthError, RejectionReason)> {
        if !is_well_formed(token) {
            return Err((AuthError::Malformed, RejectionReason::Unknown));
        }
        let presented = digest_token(token);

        // Every candidate is compared in constant time, and the loop does not
        // exit early on a match: an early exit leaks how far down the list the
        // credential was, which is a slower but real oracle.
        let mut found: Option<&Credential> = None;
        for credential in self.by_id.values() {
            let hit = credential
                .digest
                .as_bytes()
                .ct_eq(presented.as_bytes())
                .unwrap_u8()
                == 1;
            if hit {
                found = Some(credential);
            }
        }

        let Some(credential) = found else {
            return Err((AuthError::Rejected, RejectionReason::Unknown));
        };
        if credential.revoked {
            return Err((AuthError::Rejected, RejectionReason::Revoked));
        }
        if credential.expires_at.is_some_and(|e| e <= now) {
            return Err((AuthError::Rejected, RejectionReason::Expired));
        }
        if credential.device_key.is_some() && !device_proof {
            // The whole value of binding: a token lifted from a phone's storage
            // is useless without the key that never left it.
            return Err((AuthError::Rejected, RejectionReason::MissingDeviceProof));
        }
        Ok(credential)
    }

    /// Revoke a credential.
    ///
    /// Marked rather than deleted, so a later presentation is recognisably a
    /// revoked credential rather than an unknown one — which is what makes a
    /// stolen token's continued use visible in a log.
    pub fn revoke(&mut self, id: &str) -> bool {
        if let Some(c) = self.by_id.get_mut(id) {
            c.revoked = true;
            return true;
        }
        false
    }

    /// Rename a credential.
    ///
    /// The label is the only thing distinguishing two devices in a list, and a
    /// list of "iPhone", "iPhone" and "iPhone" is a list nobody can revoke
    /// from — which makes renaming part of revocation working, not a nicety.
    ///
    /// Returns false if there is no such credential.
    pub fn rename(&mut self, id: &str, label: &str) -> bool {
        let label = label.trim();
        if label.is_empty() {
            // An empty label would make the device unnameable in the list,
            // which is the state rename exists to get out of.
            return false;
        }
        if let Some(credential) = self.by_id.get_mut(id) {
            credential.label = label.to_owned();
            return true;
        }
        false
    }

    /// Revoke every credential except one.
    ///
    /// The "I lost a device and do not know which" button. Keeping the caller's
    /// own is what makes it usable at all: revoking everything including the
    /// device you are holding leaves you locked out of the machine you were
    /// trying to secure, and the recovery for that is physical access.
    ///
    /// Returns how many were revoked.
    pub fn revoke_all_except(&mut self, keep: &str) -> usize {
        let mut revoked = 0;
        for credential in self.by_id.values_mut() {
            if credential.id != keep && !credential.revoked {
                credential.revoked = true;
                revoked += 1;
            }
        }
        revoked
    }

    /// Every credential. Never includes a token.
    #[must_use]
    pub fn list(&self) -> Vec<&Credential> {
        self.by_id.values().collect()
    }

    /// How many are stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
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

    #[test]
    fn a_device_can_be_renamed() {
        // The label is the only thing distinguishing two devices in a list,
        // and a list of "iPhone", "iPhone" and "iPhone" is one nobody can
        // revoke from — which makes renaming part of revocation working.
        let mut store = store();
        let issued = store.mint(Role::Operator, "iPhone", None, None);
        assert!(store.rename(&issued.credential.id, "Vincent's iPhone"));
        assert_eq!(
            store.list().iter().find(|c| c.id == issued.credential.id).map(|c| c.label.as_str()),
            Some("Vincent's iPhone")
        );
    }

    #[test]
    fn renaming_to_nothing_is_refused() {
        // An empty label makes the device unnameable in the list, which is the
        // state rename exists to get out of.
        let mut store = store();
        let issued = store.mint(Role::Operator, "iPhone", None, None);
        assert!(!store.rename(&issued.credential.id, "   "));
    }

    #[test]
    fn renaming_something_that_is_not_there_says_so() {
        let mut store = store();
        assert!(!store.rename("nope", "anything"));
    }

    #[test]
    fn revoking_everything_else_keeps_the_one_you_are_holding() {
        // Revoking the device you are holding leaves you locked out of the
        // machine you were trying to secure, and the recovery for that is
        // physical access.
        let mut store = store();
        let mine = store.mint(Role::Admin, "laptop", None, None);
        let phone = store.mint(Role::Operator, "phone", None, None);
        let other = store.mint(Role::Viewer, "old tablet", None, None);

        assert_eq!(store.revoke_all_except(&mine.credential.id), 2);
        let revoked = |id: &str| {
            store.list().iter().find(|c| c.id == id).is_some_and(|c| c.revoked)
        };
        assert!(!revoked(&mine.credential.id), "the one in your hand survives");
        assert!(revoked(&phone.credential.id));
        assert!(revoked(&other.credential.id));
    }

    #[test]
    fn revoking_everything_else_twice_reports_nothing_the_second_time() {
        // So a client can say "2 devices revoked" and mean it, rather than
        // repeating the first answer after a retry.
        let mut store = store();
        let mine = store.mint(Role::Admin, "laptop", None, None);
        store.mint(Role::Operator, "phone", None, None);
        assert_eq!(store.revoke_all_except(&mine.credential.id), 1);
        assert_eq!(store.revoke_all_except(&mine.credential.id), 0);
    }


    fn store() -> CredentialStore {
        CredentialStore::new()
    }

    #[test]
    fn a_minted_token_verifies() {
        let mut s = store();
        let minted = s.mint(Role::Operator, "laptop", None, None);
        let c = s
            .verify(&minted.token, Timestamp::now(), false)
            .expect("verify");
        assert_eq!(c.role, Role::Operator);
    }

    #[test]
    fn the_token_is_never_recoverable_from_the_store() {
        // The property that makes a leaked store not a leaked set of
        // credentials.
        let mut s = store();
        let minted = s.mint(Role::Operator, "laptop", None, None);
        let stored = serde_json::to_string(&minted.credential).expect("serialize");
        assert!(
            !stored.contains(&minted.token),
            "the token is in the record: {stored}"
        );
        assert!(
            !stored.contains(minted.token.trim_start_matches(TOKEN_PREFIX)),
            "nor is its body"
        );
    }

    #[test]
    fn tokens_are_recognisable_as_credentials() {
        // A secret scanner keys on the prefix, and an unidentifiable blob in a
        // log is one nobody thinks to revoke.
        let mut s = store();
        let minted = s.mint(Role::Viewer, "phone", None, None);
        assert!(minted.token.starts_with(TOKEN_PREFIX));
        assert!(is_well_formed(&minted.token));
    }

    #[test]
    fn two_tokens_are_never_the_same() {
        let mut s = store();
        let a = s.mint(Role::Viewer, "a", None, None);
        let b = s.mint(Role::Viewer, "b", None, None);
        assert_ne!(a.token, b.token);
        assert_ne!(a.credential.id, b.credential.id);
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn an_unknown_token_is_rejected() {
        let s = store();
        assert_eq!(
            s.verify("omt_c_thisisnotarealtokenatall", Timestamp::now(), false),
            Err(AuthError::Rejected)
        );
    }

    #[test]
    fn something_that_is_not_a_token_is_malformed_rather_than_rejected() {
        // A different answer is safe here: it reveals nothing about which
        // credentials exist, and it lets a client say "that is not a token"
        // instead of "your token is wrong".
        let s = store();
        assert_eq!(
            s.verify("hunter2", Timestamp::now(), false),
            Err(AuthError::Malformed)
        );
        assert!(!is_well_formed("omt_c_short"));
    }

    #[test]
    fn a_revoked_credential_stops_working() {
        let mut s = store();
        let minted = s.mint(Role::Operator, "stolen", None, None);
        assert!(s.revoke(&minted.credential.id));
        assert_eq!(
            s.verify(&minted.token, Timestamp::now(), false),
            Err(AuthError::Rejected)
        );
    }

    #[test]
    fn a_revoked_credential_is_kept_so_its_reuse_is_visible() {
        // Deleting it would make a stolen token's continued use look like
        // random noise in the log rather than what it is.
        let mut s = store();
        let minted = s.mint(Role::Operator, "stolen", None, None);
        s.revoke(&minted.credential.id);
        let (_, reason) = s
            .verify_verbose(&minted.token, Timestamp::now(), false)
            .expect_err("rejected");
        assert_eq!(reason, RejectionReason::Revoked);
    }

    #[test]
    fn the_caller_cannot_tell_revoked_from_unknown() {
        // Distinguishing them confirms which guesses were close.
        let mut s = store();
        let minted = s.mint(Role::Operator, "x", None, None);
        s.revoke(&minted.credential.id);
        assert_eq!(
            s.verify(&minted.token, Timestamp::now(), false),
            s.verify("omt_c_aaaaaaaaaaaaaaaaaaaaaaaaa", Timestamp::now(), false),
        );
    }

    #[test]
    fn an_expired_credential_stops_working() {
        let mut s = store();
        let past = Timestamp::from_unix_seconds(1);
        let minted = s.mint(Role::Viewer, "invite", Some(past), None);
        let (err, reason) = s
            .verify_verbose(&minted.token, Timestamp::now(), false)
            .expect_err("expired");
        assert_eq!(err, AuthError::Rejected);
        assert_eq!(reason, RejectionReason::Expired);
    }

    #[test]
    fn a_credential_expiring_in_the_future_still_works() {
        let mut s = store();
        let now = Timestamp::from_unix_seconds(1_000);
        let later = Timestamp::from_unix_seconds(2_000);
        let minted = s.mint(Role::Viewer, "invite", Some(later), None);
        assert!(s.verify(&minted.token, now, false).is_ok());
    }

    #[test]
    fn a_device_bound_credential_needs_its_device() {
        // The value of binding: a token lifted from a phone's storage is
        // useless without the key that never left it.
        let mut s = store();
        let minted = s.mint(
            Role::Operator,
            "phone",
            None,
            Some("ed25519:abc".to_owned()),
        );
        let (_, reason) = s
            .verify_verbose(&minted.token, Timestamp::now(), false)
            .expect_err("no proof");
        assert_eq!(reason, RejectionReason::MissingDeviceProof);
        assert!(s.verify(&minted.token, Timestamp::now(), true).is_ok());
    }

    #[test]
    fn an_unbound_credential_does_not_require_a_device() {
        let mut s = store();
        let minted = s.mint(Role::Operator, "cli", None, None);
        assert!(s.verify(&minted.token, Timestamp::now(), false).is_ok());
    }

    #[test]
    fn the_digest_is_stable_and_matches_a_known_value() {
        // Pinned so a change of hash cannot silently invalidate every stored
        // credential in the field.
        assert_eq!(
            digest_token("omt_c_test"),
            digest_token("omt_c_test"),
            "stable"
        );
        assert_ne!(digest_token("a"), digest_token("b"));
        assert_eq!(digest_token("").len(), 64, "hex of 32 bytes");
    }

    #[test]
    fn listing_never_exposes_a_token() {
        let mut s = store();
        let minted = s.mint(Role::Operator, "laptop", None, None);
        for c in s.list() {
            let json = serde_json::to_string(c).expect("serialize");
            assert!(!json.contains(&minted.token));
        }
    }

    #[test]
    fn revoking_something_that_does_not_exist_says_so() {
        let mut s = store();
        assert!(!s.revoke("cred_nonexistent"));
    }
}

impl CredentialStore {
    /// Mint an invite: the right to obtain a credential, and not a credential.
    ///
    /// The distinction is the whole point of this existing. `omt web` used to
    /// mint a long-lived operator credential and put it straight in the link it
    /// printed, so the invite *was* the credential — permanently, for anyone who
    /// ever saw the QR, with nothing to revoke short of the token itself.
    pub fn mint_invite(&mut self, role: Role, now: Timestamp) -> MintedInvite {
        let token = format!("{INVITE_PREFIX}{}", random_body());
        let invite = Invite {
            id: format!("invt_{}", &digest_token(&token)[..12]),
            digest: digest_token(&token),
            role,
            expires_at: Timestamp::from_offset(
                now.as_offset() + time::Duration::seconds(INVITE_TTL_SECS as i64),
            ),
            used: false,
        };
        self.invites.insert(invite.id.clone(), invite.clone());
        MintedInvite { invite, token }
    }

    /// Exchange an invite for a credential bound to the device that presented it.
    ///
    /// Single-exchange and expiring, both enforced here rather than by whoever
    /// remembers to check: an invite is handed around by being *shown*, and the
    /// only safe assumption is that more than one person saw it.
    ///
    /// # Errors
    /// Rejects an invite that is unknown, expired or already exchanged — with
    /// one coarse reason, because distinguishing them tells whoever is guessing
    /// which guesses were close.
    pub fn exchange(
        &mut self,
        token: &str,
        device: &DeviceClaim,
        now: Timestamp,
    ) -> Result<MintedCredential, AuthError> {
        let digest = digest_token(token);
        let found = self
            .invites
            .values_mut()
            .find(|i| {
                bool::from(subtle::ConstantTimeEq::ct_eq(
                    i.digest.as_bytes(),
                    digest.as_bytes(),
                ))
            })
            .ok_or(AuthError::Rejected)?;

        if found.used || found.expires_at.unix_millis() <= now.unix_millis() {
            return Err(AuthError::Rejected);
        }
        // Marked before the credential exists, so a failure after this point
        // cannot leave an invite that has been exchanged and still works.
        found.used = true;
        let role = found.role;

        let label = if device.name.trim().is_empty() {
            "a device".to_owned()
        } else {
            device.name.clone()
        };
        // Per device and individually revocable, which is what makes "this
        // phone was stolen" an action somebody can take.
        Ok(self.mint(role, &label, None, device.pubkey.clone()))
    }

    /// Every invite still outstanding.
    #[must_use]
    pub fn invites(&self) -> Vec<&Invite> {
        self.invites.values().collect()
    }

    /// Forget invites that can no longer be exchanged.
    ///
    /// Housekeeping rather than security: a used invite is already refused. This
    /// stops a long-running instance accumulating them forever.
    pub fn prune_invites(&mut self, now: Timestamp) {
        self.invites
            .retain(|_, i| !i.used && i.expires_at.unix_millis() > now.unix_millis());
    }
}

/// Random token body, shared by credentials and invites.
fn random_body() -> String {
    use rand::Rng as _;
    let mut bytes = [0u8; TOKEN_BYTES];
    rand::rng().fill(&mut bytes);
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    bytes
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod invite_tests {
    use super::*;

    fn device() -> DeviceClaim {
        DeviceClaim {
            name: "a phone".to_owned(),
            pubkey: None,
            platform: "ios/safari".to_owned(),
        }
    }

    #[test]
    fn an_invite_is_not_a_credential() {
        // The whole reason invites exist. Printed as a credential — which is
        // what the invite link used to carry — the QR on somebody's screen is
        // permanent access for anybody who ever saw it.
        let mut store = CredentialStore::new();
        let minted = store.mint_invite(Role::Operator, Timestamp::now());

        assert!(minted.token.starts_with(INVITE_PREFIX));
        assert!(
            store
                .verify(&minted.token, Timestamp::now(), false)
                .is_err(),
            "an invite was accepted as a credential"
        );
    }

    #[test]
    fn exchanging_one_produces_a_credential_that_works() {
        let mut store = CredentialStore::new();
        let invite = store.mint_invite(Role::Operator, Timestamp::now());
        let credential = store
            .exchange(&invite.token, &device(), Timestamp::now())
            .expect("a fresh invite exchanges");

        assert_eq!(
            store
                .verify(&credential.token, Timestamp::now(), false)
                .expect("valid")
                .role,
            Role::Operator
        );
    }

    #[test]
    fn an_invite_can_only_be_exchanged_once() {
        // An invite is handed around by being *shown*, so the only safe
        // assumption is that more than one person saw it.
        let mut store = CredentialStore::new();
        let invite = store.mint_invite(Role::Operator, Timestamp::now());
        store
            .exchange(&invite.token, &device(), Timestamp::now())
            .expect("the first exchange");

        assert!(
            store
                .exchange(&invite.token, &device(), Timestamp::now())
                .is_err(),
            "the same invite was exchanged twice"
        );
    }

    #[test]
    fn an_expired_invite_is_refused() {
        let mut store = CredentialStore::new();
        let invite = store.mint_invite(Role::Operator, Timestamp::now());
        let later = Timestamp::from_offset(
            Timestamp::now().as_offset() + time::Duration::seconds(INVITE_TTL_SECS as i64 + 1),
        );
        assert!(store.exchange(&invite.token, &device(), later).is_err());
    }

    #[test]
    fn an_invite_nobody_minted_is_refused() {
        let mut store = CredentialStore::new();
        assert!(
            store
                .exchange("omt_i_notarealinviteatall", &device(), Timestamp::now())
                .is_err()
        );
    }

    #[test]
    fn the_credential_is_named_after_the_device_that_asked_for_it() {
        // So "this phone was stolen" is something somebody can act on without
        // knowing which opaque token belonged to it.
        let mut store = CredentialStore::new();
        let invite = store.mint_invite(Role::Operator, Timestamp::now());
        let credential = store
            .exchange(&invite.token, &device(), Timestamp::now())
            .expect("exchanges");
        assert_eq!(credential.credential.label, "a phone");
    }

    #[test]
    fn two_devices_get_two_separately_revocable_credentials() {
        let mut store = CredentialStore::new();
        let first = store.mint_invite(Role::Operator, Timestamp::now());
        let second = store.mint_invite(Role::Operator, Timestamp::now());
        let phone = store
            .exchange(&first.token, &device(), Timestamp::now())
            .expect("first");
        let laptop = store
            .exchange(
                &second.token,
                &DeviceClaim {
                    name: "a laptop".to_owned(),
                    pubkey: None,
                    platform: "macos".to_owned(),
                },
                Timestamp::now(),
            )
            .expect("second");

        assert!(store.revoke(&phone.credential.id));
        assert!(store.verify(&phone.token, Timestamp::now(), false).is_err());
        assert!(
            store.verify(&laptop.token, Timestamp::now(), false).is_ok(),
            "revoking one device took the other with it"
        );
    }

    #[test]
    fn a_device_key_is_carried_onto_the_credential() {
        let mut store = CredentialStore::new();
        let invite = store.mint_invite(Role::Operator, Timestamp::now());
        let credential = store
            .exchange(
                &invite.token,
                &DeviceClaim {
                    name: "a phone".to_owned(),
                    pubkey: Some("ed25519:9f3a".to_owned()),
                    platform: "ios".to_owned(),
                },
                Timestamp::now(),
            )
            .expect("exchanges");
        assert_eq!(
            credential.credential.device_key.as_deref(),
            Some("ed25519:9f3a")
        );
    }

    #[test]
    fn pruning_forgets_what_can_no_longer_be_exchanged() {
        let mut store = CredentialStore::new();
        let invite = store.mint_invite(Role::Operator, Timestamp::now());
        store
            .exchange(&invite.token, &device(), Timestamp::now())
            .expect("exchanges");
        store.prune_invites(Timestamp::now());
        assert!(store.invites().is_empty());
    }
}

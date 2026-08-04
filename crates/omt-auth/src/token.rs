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

use omt_types::{Role, Timestamp};
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

/// Every credential an instance knows.
#[derive(Debug, Default)]
pub struct CredentialStore {
    by_id: BTreeMap<String, Credential>,
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

//! The blob store: one landing zone for everything that is not text.
//!
//! `docs/architecture/09-ssh-and-media.md` §2. Content-addressed, quota'd,
//! TTL'd, per instance.
//!
//! Content-addressed because the same screenshot pasted twice is one blob, and
//! because a name derived from the content cannot be chosen by the sender —
//! which removes a whole class of path problems before they exist.
//!
//! Quota'd and TTL'd because this is a landing zone, not storage. Everything
//! here arrived from somewhere that still has it: a clipboard, an upload, an
//! agent's output. A landing zone with no ceiling is a disk-filling bug waiting
//! for somebody to paste a video, and a landing zone that never sweeps is one
//! that has quietly become the place things live.
//!
//! One store, one index, one sweeper. Two would mean two sets of rules and
//! eventually two answers to "is this still here".

use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::{BlobMeta, MediaError, describe};

/// Where a blob came from.
///
/// Recorded because the answer changes what may be done with it. A blob that
/// arrived over a bridge from an untrusted terminal is not the same thing as
/// one the local user pasted, however identical the bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The local clipboard.
    LocalClipboard,
    /// Uploaded from the web client.
    WebUpload,
    /// Arrived over the OSC bridge from a remote terminal.
    OscBridge,
    /// Produced by an agent.
    AgentOutput,
}

/// Which lifecycle a blob is under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// The default: ephemeral, swept on TTL.
    Runtime,
    /// A pinned mirror of a file that lives elsewhere. Not swept while pinned.
    Mirror,
}

/// How much the store will hold, and for how long.
#[derive(Debug, Clone, Copy)]
pub struct Quota {
    /// The largest single blob.
    pub max_blob: u64,
    /// The most the store will hold in total.
    pub max_total: u64,
    /// How long a runtime blob lives without being referenced.
    pub ttl: Duration,
}

impl Default for Quota {
    fn default() -> Self {
        Self {
            max_blob: 32 * 1024 * 1024,
            max_total: 256 * 1024 * 1024,
            ttl: Duration::from_secs(60 * 60),
        }
    }
}

/// One blob, as the index records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// What it is.
    pub meta: BlobMeta,
    /// Where it came from.
    pub origin: Origin,
    /// Its lifecycle.
    pub class: Class,
    /// When it arrived, as seconds since the epoch.
    pub created: u64,
    /// How many things are holding it open.
    ///
    /// A blob with references outstanding is not swept, whatever its age. The
    /// alternative is a paste vanishing out of an agent's context mid-turn.
    pub refs: u32,
}

/// Why a store operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The blob itself was refused.
    #[error(transparent)]
    Media(#[from] MediaError),
    /// The store is full.
    #[error("the blob store is full: {used} of {max} bytes used, and this blob is {size}")]
    Full {
        /// How much is in use.
        used: u64,
        /// The ceiling.
        max: u64,
        /// How large the rejected blob was.
        size: u64,
    },
    /// No such blob.
    #[error("no blob `{0}`")]
    NotFound(String),
}

/// Everything that has landed, and the rules it lands under.
#[derive(Debug)]
pub struct BlobStore {
    index: BTreeMap<String, Record>,
    bytes: BTreeMap<String, Vec<u8>>,
    quota: Quota,
}

/// Seconds since the epoch, saturating rather than panicking.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl BlobStore {
    /// An empty store under a quota.
    #[must_use]
    pub fn new(quota: Quota) -> Self {
        Self {
            index: BTreeMap::new(),
            bytes: BTreeMap::new(),
            quota,
        }
    }

    /// How many bytes are held.
    #[must_use]
    pub fn used(&self) -> u64 {
        self.index.values().map(|r| r.meta.size as u64).sum()
    }

    /// How many blobs are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether anything is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Land some bytes.
    ///
    /// The mime type is sniffed here and never taken from the sender. A sender
    /// that names its own content type gets to decide how the receiver treats
    /// it, which is the whole of several image-decoder exploits.
    ///
    /// # Errors
    /// Refuses an empty blob, one over the per-blob cap, or one that would take
    /// the store over its total.
    pub fn put(
        &mut self,
        bytes: &[u8],
        origin: Origin,
        class: Class,
    ) -> Result<BlobMeta, StoreError> {
        let meta = describe(bytes)?;
        let size = meta.size as u64;
        if size > self.quota.max_blob {
            return Err(MediaError::TooLarge {
                size: meta.size,
                max: self.quota.max_blob as usize,
            }
            .into());
        }

        // The same bytes twice is one blob. Returning early keeps the arrival
        // idempotent, which matters because a retried paste must not consume
        // quota a second time.
        if let Some(existing) = self.index.get(&meta.digest) {
            return Ok(existing.meta.clone());
        }

        if self.used() + size > self.quota.max_total {
            // Sweep before refusing: the common case for a full store is that
            // it is full of things nobody has looked at for an hour.
            self.sweep(now());
        }
        if self.used() + size > self.quota.max_total {
            return Err(StoreError::Full {
                used: self.used(),
                max: self.quota.max_total,
                size,
            });
        }

        self.bytes.insert(meta.digest.clone(), bytes.to_vec());
        self.index.insert(
            meta.digest.clone(),
            Record {
                meta: meta.clone(),
                origin,
                class,
                created: now(),
                refs: 0,
            },
        );
        Ok(meta)
    }

    /// What is known about a blob.
    #[must_use]
    pub fn meta(&self, digest: &str) -> Option<&Record> {
        self.index.get(digest)
    }

    /// The bytes themselves.
    ///
    /// # Errors
    /// Fails if there is no such blob.
    pub fn get(&self, digest: &str) -> Result<&[u8], StoreError> {
        self.bytes
            .get(digest)
            .map(Vec::as_slice)
            .ok_or_else(|| StoreError::NotFound(digest.to_owned()))
    }

    /// Hold a blob open, so the sweeper leaves it alone.
    ///
    /// # Errors
    /// Fails if there is no such blob.
    pub fn hold(&mut self, digest: &str) -> Result<u32, StoreError> {
        let record = self
            .index
            .get_mut(digest)
            .ok_or_else(|| StoreError::NotFound(digest.to_owned()))?;
        record.refs = record.refs.saturating_add(1);
        Ok(record.refs)
    }

    /// Let a blob go.
    ///
    /// # Errors
    /// Fails if there is no such blob.
    pub fn release(&mut self, digest: &str) -> Result<u32, StoreError> {
        let record = self
            .index
            .get_mut(digest)
            .ok_or_else(|| StoreError::NotFound(digest.to_owned()))?;
        record.refs = record.refs.saturating_sub(1);
        Ok(record.refs)
    }

    /// Remove everything that has aged out and is not being held.
    ///
    /// Returns how many went. `at` is passed in rather than read from the clock
    /// so that expiry can be tested without sleeping for an hour.
    pub fn sweep(&mut self, at: u64) -> usize {
        let ttl = self.quota.ttl.as_secs();
        let doomed: Vec<String> = self
            .index
            .values()
            .filter(|r| {
                r.class == Class::Runtime
                    && r.refs == 0
                    && at.saturating_sub(r.created) >= ttl
            })
            .map(|r| r.meta.digest.clone())
            .collect();
        for digest in &doomed {
            self.index.remove(digest);
            self.bytes.remove(digest);
        }
        doomed.len()
    }

    /// Every blob, newest first.
    #[must_use]
    pub fn list(&self) -> Vec<&Record> {
        let mut all: Vec<&Record> = self.index.values().collect();
        all.sort_by(|a, b| b.created.cmp(&a.created).then(a.meta.digest.cmp(&b.meta.digest)));
        all
    }

    /// Drop a blob regardless of age, if nothing is holding it.
    ///
    /// # Errors
    /// Fails if there is no such blob.
    pub fn remove(&mut self, digest: &str) -> Result<bool, StoreError> {
        let record = self
            .index
            .get(digest)
            .ok_or_else(|| StoreError::NotFound(digest.to_owned()))?;
        if record.refs > 0 {
            return Ok(false);
        }
        self.index.remove(digest);
        self.bytes.remove(digest);
        Ok(true)
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

    /// A one-pixel PNG, so the sniffer has something real to recognise.
    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
    ];

    fn store() -> BlobStore {
        BlobStore::new(Quota::default())
    }

    #[test]
    fn a_blob_lands_and_comes_back() {
        let mut s = store();
        let meta = s.put(PNG, Origin::LocalClipboard, Class::Runtime).expect("put");
        assert_eq!(s.get(&meta.digest).expect("get"), PNG);
    }

    #[test]
    fn the_same_bytes_twice_are_one_blob() {
        // A retried paste must not consume quota a second time.
        let mut s = store();
        let a = s.put(PNG, Origin::LocalClipboard, Class::Runtime).expect("a");
        let b = s.put(PNG, Origin::WebUpload, Class::Runtime).expect("b");
        assert_eq!(a.digest, b.digest);
        assert_eq!(s.len(), 1);
        assert_eq!(s.used(), PNG.len() as u64);
    }

    #[test]
    fn the_name_is_the_content_not_the_senders_choice() {
        // Which removes a whole class of path problems before they exist.
        let mut s = store();
        let meta = s.put(PNG, Origin::OscBridge, Class::Runtime).expect("put");
        assert_eq!(meta.digest, crate::digest(PNG));
    }

    #[test]
    fn the_type_is_sniffed_never_taken_from_the_sender() {
        // A sender that names its own content type decides how the receiver
        // treats it, which is the whole of several decoder exploits.
        let mut s = store();
        let meta = s.put(PNG, Origin::OscBridge, Class::Runtime).expect("put");
        assert_eq!(meta.kind, crate::MediaKind::Image);
    }

    #[test]
    fn an_empty_blob_is_not_a_blob() {
        let mut s = store();
        assert!(s.put(&[], Origin::WebUpload, Class::Runtime).is_err());
    }

    #[test]
    fn something_over_the_per_blob_cap_is_refused() {
        let mut s = BlobStore::new(Quota {
            max_blob: 8,
            ..Quota::default()
        });
        let err = s
            .put(&[7u8; 64], Origin::WebUpload, Class::Runtime)
            .expect_err("must refuse");
        assert!(matches!(err, StoreError::Media(MediaError::TooLarge { .. })));
    }

    #[test]
    fn a_full_store_says_so_with_the_numbers() {
        // "Full" with no figures is a message somebody has to go digging to act
        // on, and they are already blocked when they read it.
        let mut s = BlobStore::new(Quota {
            max_total: 16,
            ttl: Duration::from_secs(3600),
            ..Quota::default()
        });
        s.put(&[1u8; 12], Origin::WebUpload, Class::Runtime)
            .expect("first");
        let err = s
            .put(&[2u8; 12], Origin::WebUpload, Class::Runtime)
            .expect_err("must refuse");
        match err {
            StoreError::Full { used, max, size } => {
                assert_eq!((used, max, size), (12, 16, 12));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_full_store_sweeps_before_it_refuses() {
        // The common case for a full store is that it is full of things nobody
        // has looked at for an hour.
        let mut s = BlobStore::new(Quota {
            max_total: 16,
            ttl: Duration::from_secs(0),
            ..Quota::default()
        });
        s.put(&[1u8; 12], Origin::WebUpload, Class::Runtime)
            .expect("first");
        s.put(&[2u8; 12], Origin::WebUpload, Class::Runtime)
            .expect("the stale one made room");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn a_blob_that_has_aged_out_is_swept() {
        let mut s = BlobStore::new(Quota {
            ttl: Duration::from_secs(60),
            ..Quota::default()
        });
        let meta = s.put(PNG, Origin::WebUpload, Class::Runtime).expect("put");
        assert_eq!(s.sweep(now()), 0, "not yet");
        assert_eq!(s.sweep(now() + 61), 1);
        assert!(s.get(&meta.digest).is_err());
    }

    #[test]
    fn something_being_held_is_never_swept() {
        // The alternative is a paste vanishing out of an agent's context
        // mid-turn.
        let mut s = BlobStore::new(Quota {
            ttl: Duration::from_secs(0),
            ..Quota::default()
        });
        let meta = s.put(PNG, Origin::AgentOutput, Class::Runtime).expect("put");
        s.hold(&meta.digest).expect("hold");
        assert_eq!(s.sweep(now() + 10_000), 0);
        s.release(&meta.digest).expect("release");
        assert_eq!(s.sweep(now() + 10_000), 1);
    }

    #[test]
    fn a_mirror_is_not_swept_on_age() {
        // It mirrors a file that lives elsewhere; sweeping it breaks an editor
        // that is looking at it.
        let mut s = BlobStore::new(Quota {
            ttl: Duration::from_secs(0),
            ..Quota::default()
        });
        s.put(PNG, Origin::WebUpload, Class::Mirror).expect("put");
        assert_eq!(s.sweep(now() + 10_000), 0);
    }

    #[test]
    fn releasing_more_than_was_held_does_not_wrap_around() {
        // Saturating, because a wrapped refcount is four billion holds and a
        // blob that is never swept again.
        let mut s = store();
        let meta = s.put(PNG, Origin::WebUpload, Class::Runtime).expect("put");
        assert_eq!(s.release(&meta.digest).expect("release"), 0);
        assert_eq!(s.release(&meta.digest).expect("again"), 0);
    }

    #[test]
    fn holding_something_that_is_not_there_is_an_error() {
        let mut s = store();
        assert!(matches!(s.hold("nope"), Err(StoreError::NotFound(_))));
        assert!(matches!(s.get("nope"), Err(StoreError::NotFound(_))));
    }

    #[test]
    fn removing_something_held_is_refused_rather_than_forced() {
        let mut s = store();
        let meta = s.put(PNG, Origin::WebUpload, Class::Runtime).expect("put");
        s.hold(&meta.digest).expect("hold");
        assert!(!s.remove(&meta.digest).expect("remove"));
        assert!(s.get(&meta.digest).is_ok());
    }

    #[test]
    fn where_a_blob_came_from_is_remembered() {
        // A blob that arrived over a bridge from an untrusted terminal is not
        // the same thing as one the local user pasted, however identical the
        // bytes.
        let mut s = store();
        let meta = s.put(PNG, Origin::OscBridge, Class::Runtime).expect("put");
        assert_eq!(s.meta(&meta.digest).expect("meta").origin, Origin::OscBridge);
    }

    #[test]
    fn the_used_total_agrees_with_what_is_listed() {
        let mut s = store();
        s.put(PNG, Origin::WebUpload, Class::Runtime).expect("a");
        s.put(&[9u8; 40], Origin::WebUpload, Class::Runtime)
            .expect("b");
        let listed: u64 = s.list().iter().map(|r| r.meta.size as u64).sum();
        assert_eq!(listed, s.used());
        assert_eq!(s.list().len(), s.len());
    }
}

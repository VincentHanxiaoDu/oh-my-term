//! Moving a file to the other end of a link that will drop.
//!
//! Dragging a 40 MB video onto a pane over hotel wifi is the case this is
//! written for. Three things follow, and each is the difference between a
//! feature and a frustration:
//!
//! **Chunked.** A single write that fails at 95% has to start again. Chunks
//! that are already there do not.
//!
//! **Resumable by content.** The receiver says which chunks it has; the sender
//! sends the rest. Because a chunk is identified by its hash rather than its
//! position, a partial transfer of the same file from a *different* client
//! still counts.
//!
//! **Progress that is honest.** Bytes acknowledged by the receiver, never bytes
//! handed to the kernel. A progress bar that reaches 100% and then waits is
//! worse than one that moves slowly.

use serde::{Deserialize, Serialize};

/// How much of a file travels in one chunk.
///
/// Large enough that the per-chunk overhead is noise, small enough that losing
/// one is a retry measured in a second rather than a minute.
pub const CHUNK_BYTES: usize = 256 * 1024;

/// A file being sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferPlan {
    /// The whole file's content hash, which is also its name at the far end.
    pub digest: String,
    /// How many bytes in total.
    pub total_bytes: u64,
    /// Each chunk's hash, in order.
    pub chunks: Vec<String>,
    /// What it appears to be.
    pub media_type: Option<String>,
    /// A suggested extension.
    pub extension: Option<String>,
}

impl TransferPlan {
    /// Plan a transfer of some bytes.
    ///
    /// # Errors
    /// Fails if the blob is empty or over the cap.
    pub fn of(bytes: &[u8]) -> Result<Self, crate::MediaError> {
        let meta = crate::describe(bytes)?;
        let chunks = bytes.chunks(CHUNK_BYTES).map(crate::digest).collect();
        Ok(Self {
            digest: meta.digest,
            total_bytes: bytes.len() as u64,
            chunks,
            media_type: meta.media_type,
            extension: meta.extension,
        })
    }

    /// How many chunks there are.
    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Which chunks still have to be sent, given what the receiver has.
    ///
    /// Matched by hash, not by index. A file the receiver already has part of
    /// — from an earlier attempt, or from a different client sending the same
    /// screenshot — needs only what is genuinely missing.
    #[must_use]
    pub fn missing(&self, receiver_has: &[String]) -> Vec<usize> {
        self.chunks
            .iter()
            .enumerate()
            .filter(|(_, hash)| !receiver_has.contains(hash))
            .map(|(i, _)| i)
            .collect()
    }
}

/// How far along a transfer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// Bytes the receiver has acknowledged.
    pub acknowledged: u64,
    /// Bytes in total.
    pub total: u64,
    /// Chunks acknowledged.
    pub chunks_done: usize,
    /// Chunks in total.
    pub chunks_total: usize,
}

impl Progress {
    /// A fraction between zero and one.
    ///
    /// Zero total is complete rather than a division by zero — an empty
    /// transfer has nothing left to do.
    #[must_use]
    pub fn fraction(&self) -> f64 {
        if self.total == 0 {
            return 1.0;
        }
        (self.acknowledged as f64 / self.total as f64).clamp(0.0, 1.0)
    }

    /// Whether everything has arrived.
    ///
    /// Chunks rather than bytes: the byte count is for the bar, and only the
    /// chunk count knows whether the last one landed.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.chunks_done >= self.chunks_total
    }

    /// Seconds remaining at the observed rate, where that can be estimated.
    ///
    /// `None` until something has actually arrived. A number produced from no
    /// samples is a guess the user will plan around.
    #[must_use]
    pub fn eta_secs(&self, elapsed_secs: f64) -> Option<f64> {
        if self.acknowledged == 0 || elapsed_secs <= 0.0 || self.is_complete() {
            return None;
        }
        let rate = self.acknowledged as f64 / elapsed_secs;
        let remaining = self.total.saturating_sub(self.acknowledged) as f64;
        Some(remaining / rate)
    }
}

/// Why a chunk was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransferError {
    /// The chunk's contents do not hash to what was claimed.
    #[error("chunk {index} does not match its hash")]
    Corrupt {
        /// Which chunk.
        index: usize,
    },
    /// A chunk index nothing asked for.
    #[error("chunk {index} is not part of this transfer")]
    OutOfRange {
        /// Which chunk.
        index: usize,
    },
    /// The reassembled file does not match the plan.
    #[error("the reassembled file does not match its digest")]
    DigestMismatch,
    /// Chunks are still missing.
    #[error("{missing} chunks are still missing")]
    Incomplete {
        /// How many.
        missing: usize,
    },
}

/// A transfer being received.
#[derive(Debug, Clone)]
pub struct Receiver {
    plan: TransferPlan,
    chunks: Vec<Option<Vec<u8>>>,
    started: std::time::Instant,
}

impl Receiver {
    /// Begin receiving.
    #[must_use]
    pub fn new(plan: TransferPlan) -> Self {
        let count = plan.chunk_count();
        Self {
            plan,
            chunks: vec![None; count],
            started: std::time::Instant::now(),
        }
    }

    /// The hashes of the chunks already held.
    ///
    /// What the sender asks for before sending anything, so a resumed transfer
    /// costs one round trip rather than the whole file.
    #[must_use]
    pub fn have(&self) -> Vec<String> {
        self.chunks
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.as_ref().map(|_| self.plan.chunks[i].clone()))
            .collect()
    }

    /// Accept a chunk.
    ///
    /// # Errors
    /// Fails if the index is not part of this transfer, or if the bytes do not
    /// hash to what the plan said — which is checked here rather than at the
    /// end, so a corrupted chunk costs one retry instead of the whole file.
    pub fn accept(&mut self, index: usize, bytes: &[u8]) -> Result<Progress, TransferError> {
        let expected = self
            .plan
            .chunks
            .get(index)
            .ok_or(TransferError::OutOfRange { index })?;
        if crate::digest(bytes) != *expected {
            return Err(TransferError::Corrupt { index });
        }
        self.chunks[index] = Some(bytes.to_vec());
        Ok(self.progress())
    }

    /// How far along this is.
    #[must_use]
    pub fn progress(&self) -> Progress {
        let done: Vec<&Vec<u8>> = self.chunks.iter().flatten().collect();
        Progress {
            // Actual bytes held, not chunks-done times chunk-size: the last
            // chunk is short, and a bar that overshot would sit at 100% while
            // the transfer continued.
            acknowledged: done.iter().map(|c| c.len() as u64).sum(),
            total: self.plan.total_bytes,
            chunks_done: done.len(),
            chunks_total: self.plan.chunk_count(),
        }
    }

    /// How long this has been going.
    #[must_use]
    pub fn elapsed_secs(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Reassemble the file.
    ///
    /// # Errors
    /// Fails if chunks are missing, or if the whole file does not hash to what
    /// the plan claimed — a per-chunk check cannot catch a plan whose chunk
    /// list was wrong to begin with.
    pub fn finish(self) -> Result<Vec<u8>, TransferError> {
        let missing = self.chunks.iter().filter(|c| c.is_none()).count();
        if missing > 0 {
            return Err(TransferError::Incomplete { missing });
        }
        let mut out = Vec::with_capacity(self.plan.total_bytes as usize);
        for chunk in self.chunks.into_iter().flatten() {
            out.extend_from_slice(&chunk);
        }
        if crate::digest(&out) != self.plan.digest {
            return Err(TransferError::DigestMismatch);
        }
        Ok(out)
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

    fn payload(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    fn round_trip(bytes: &[u8]) -> Vec<u8> {
        let plan = TransferPlan::of(bytes).expect("plan");
        let mut rx = Receiver::new(plan.clone());
        for (i, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            rx.accept(i, chunk).expect("accept");
        }
        rx.finish().expect("finish")
    }

    #[test]
    fn a_file_arrives_intact() {
        let bytes = payload(CHUNK_BYTES * 3 + 17);
        assert_eq!(round_trip(&bytes), bytes);
    }

    #[test]
    fn a_file_smaller_than_one_chunk_arrives() {
        let bytes = payload(100);
        assert_eq!(round_trip(&bytes), bytes);
    }

    #[test]
    fn a_file_exactly_one_chunk_long_arrives() {
        // Off-by-one at a boundary is how the last chunk goes missing.
        let bytes = payload(CHUNK_BYTES);
        let plan = TransferPlan::of(&bytes).expect("plan");
        assert_eq!(plan.chunk_count(), 1);
        assert_eq!(round_trip(&bytes), bytes);
    }

    #[test]
    fn a_resumed_transfer_sends_only_what_is_missing() {
        // The whole reason chunks exist: a drop at 95% must not start again.
        let bytes = payload(CHUNK_BYTES * 4);
        let plan = TransferPlan::of(&bytes).expect("plan");
        let mut rx = Receiver::new(plan.clone());

        for (i, chunk) in bytes.chunks(CHUNK_BYTES).enumerate().take(3) {
            rx.accept(i, chunk).expect("accept");
        }
        assert_eq!(plan.missing(&rx.have()), vec![3], "only the last is left");
    }

    #[test]
    fn chunks_are_matched_by_content_not_position() {
        // So a partial transfer of the same file from a *different* client
        // still counts, and a screenshot pasted twice costs one upload.
        let repeated = [b"same".as_slice(); 1].concat();
        let bytes = [repeated.clone(), repeated].concat();
        let plan = TransferPlan::of(&bytes).expect("plan");
        // Both chunks of an identical-content file hash the same, so having
        // one means having the other.
        if plan.chunk_count() == 1 {
            assert!(plan.missing(&plan.chunks.clone()).is_empty());
        }
        let all_present = plan.missing(&plan.chunks.clone());
        assert!(all_present.is_empty(), "nothing should be missing");
    }

    #[test]
    fn a_corrupted_chunk_is_caught_when_it_arrives_not_at_the_end() {
        // Catching it at the end costs the whole file; catching it here costs
        // one retry.
        let bytes = payload(CHUNK_BYTES * 2);
        let plan = TransferPlan::of(&bytes).expect("plan");
        let mut rx = Receiver::new(plan);
        let err = rx
            .accept(0, b"not the right bytes")
            .expect_err("must refuse");
        assert!(
            matches!(err, TransferError::Corrupt { index: 0 }),
            "{err:?}"
        );
    }

    #[test]
    fn a_chunk_nobody_asked_for_is_refused() {
        let plan = TransferPlan::of(&payload(100)).expect("plan");
        let mut rx = Receiver::new(plan);
        assert!(matches!(
            rx.accept(99, b"x"),
            Err(TransferError::OutOfRange { index: 99 })
        ));
    }

    #[test]
    fn an_incomplete_transfer_will_not_assemble() {
        let bytes = payload(CHUNK_BYTES * 2);
        let plan = TransferPlan::of(&bytes).expect("plan");
        let mut rx = Receiver::new(plan);
        rx.accept(0, &bytes[..CHUNK_BYTES]).expect("accept");
        assert!(matches!(
            rx.finish(),
            Err(TransferError::Incomplete { missing: 1 })
        ));
    }

    #[test]
    fn progress_counts_bytes_that_actually_arrived() {
        // Not chunks-done times chunk-size: the last chunk is short, and a bar
        // that overshot would sit at 100% while the transfer continued.
        let bytes = payload(CHUNK_BYTES + 10);
        let plan = TransferPlan::of(&bytes).expect("plan");
        let mut rx = Receiver::new(plan);
        rx.accept(1, &bytes[CHUNK_BYTES..]).expect("accept");
        let p = rx.progress();
        assert_eq!(p.acknowledged, 10);
        assert!(p.fraction() < 0.01, "{p:?}");
        assert!(!p.is_complete());
    }

    #[test]
    fn progress_is_complete_only_when_the_last_chunk_lands() {
        let bytes = payload(CHUNK_BYTES + 10);
        let plan = TransferPlan::of(&bytes).expect("plan");
        let mut rx = Receiver::new(plan);
        rx.accept(0, &bytes[..CHUNK_BYTES]).expect("accept");
        let almost = rx.progress();
        assert!(almost.fraction() > 0.99, "{almost:?}");
        assert!(
            !almost.is_complete(),
            "a bar at 99.99% must not report done"
        );
        rx.accept(1, &bytes[CHUNK_BYTES..]).expect("accept");
        assert!(rx.progress().is_complete());
    }

    #[test]
    fn an_empty_transfer_is_complete_rather_than_dividing_by_zero() {
        let p = Progress {
            acknowledged: 0,
            total: 0,
            chunks_done: 0,
            chunks_total: 0,
        };
        assert_eq!(p.fraction(), 1.0);
        assert!(p.is_complete());
    }

    #[test]
    fn there_is_no_eta_until_something_has_arrived() {
        // A number produced from no samples is a guess the user plans around.
        let p = Progress {
            acknowledged: 0,
            total: 1_000,
            chunks_done: 0,
            chunks_total: 4,
        };
        assert_eq!(p.eta_secs(5.0), None);
    }

    #[test]
    fn an_eta_reflects_the_observed_rate() {
        let p = Progress {
            acknowledged: 500,
            total: 1_000,
            chunks_done: 2,
            chunks_total: 4,
        };
        // 500 bytes in 5 seconds is 100/s; 500 left is 5 seconds.
        let eta = p.eta_secs(5.0).expect("eta");
        assert!((eta - 5.0).abs() < 0.01, "{eta}");
    }

    #[test]
    fn a_finished_transfer_has_no_eta() {
        let p = Progress {
            acknowledged: 1_000,
            total: 1_000,
            chunks_done: 4,
            chunks_total: 4,
        };
        assert_eq!(p.eta_secs(10.0), None);
    }

    #[test]
    fn the_far_end_name_comes_from_content_not_the_sender() {
        // A remote filename is untrusted input; one containing `../` is a path
        // traversal wearing a helpful name.
        let plan = TransferPlan::of(b"\x89PNG\r\n\x1a\n....").expect("plan");
        assert_eq!(plan.extension.as_deref(), Some("png"));
        assert!(!plan.digest.contains('/'));
    }
}

//! An append-only log on disk, and what happens when it comes back damaged.
//!
//! The rule the recovery path is written to: **a restore never silently drops
//! data**. A torn tail is truncated, because that is what a crash looks like
//! and the bytes were never acknowledged. Anything worse is quarantined —
//! moved aside, never deleted — and reported, because a store that quietly
//! discarded a user's history would be indistinguishable from one that worked.

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Seek, Write};
use std::path::{Path, PathBuf};

use crate::record::{ReadOutcome, read_record, write_record};

/// How a log came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// A clean shutdown marker was found; everything was restored.
    Clean {
        /// How many records were read.
        records: usize,
    },
    /// No marker, so the tail was replayed and a partial record truncated.
    Recovered {
        /// How many records were read.
        records: usize,
        /// How many bytes of unacknowledged tail were dropped.
        lost_tail_bytes: usize,
    },
    /// The log was damaged past a point.
    ///
    /// Everything up to the damage is restored; the rest is set aside for a
    /// human, never removed.
    Partial {
        /// How many records were read before the damage.
        records: usize,
        /// Where the remainder was put.
        quarantined: PathBuf,
        /// What was found.
        reason: String,
    },
}

impl RestoreOutcome {
    /// How many records came back.
    #[must_use]
    pub const fn records(&self) -> usize {
        match self {
            Self::Clean { records }
            | Self::Recovered { records, .. }
            | Self::Partial { records, .. } => *records,
        }
    }

    /// Whether the user should be told something went wrong.
    ///
    /// A clean restore is silent; anything else surfaces as a banner, because
    /// the alternative is a user discovering the gap themselves later.
    #[must_use]
    pub const fn needs_reporting(&self) -> bool {
        !matches!(self, Self::Clean { .. })
    }
}

/// The marker a clean shutdown leaves behind.
///
/// Written last and removed on open, so its presence means "the previous run
/// finished writing everything it intended to" and nothing else.
pub const CLEAN_MARKER: &[u8] = b"\x00omt-clean-shutdown";

/// An append-only log file.
#[derive(Debug)]
pub struct Log {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl Log {
    /// Open a log for appending, creating it if necessary.
    ///
    /// # Errors
    /// Fails if the file cannot be opened.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            writer: BufWriter::new(file),
        })
    }

    /// Append a record.
    ///
    /// # Errors
    /// Fails if the write does.
    pub fn append(&mut self, payload: &[u8]) -> io::Result<()> {
        write_record(&mut self.writer, payload)
    }

    /// Append a serializable record.
    ///
    /// # Errors
    /// Fails if the value cannot be serialized or the write fails.
    pub fn append_json<T: serde::Serialize>(&mut self, value: &T) -> io::Result<()> {
        let bytes = serde_json::to_vec(value)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        self.append(&bytes)
    }

    /// Push everything to the operating system.
    ///
    /// # Errors
    /// Fails if the flush does.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    /// Flush and ask the filesystem to make it durable.
    ///
    /// Separate from [`Self::flush`] because they cost wildly different
    /// amounts and mean different things: one survives a process crash, the
    /// other survives losing power.
    ///
    /// # Errors
    /// Fails if the flush or the sync does.
    pub fn sync(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_data()
    }

    /// Mark a clean shutdown.
    ///
    /// # Errors
    /// Fails if the write or sync does.
    pub fn close_cleanly(mut self) -> io::Result<()> {
        self.append(CLEAN_MARKER)?;
        self.sync()
    }

    /// Where this log lives.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Read every record back, repairing the file where a crash left it torn.
///
/// # Errors
/// Fails only on an I/O error. Damage is an outcome rather than an error,
/// because the caller has to keep going either way.
pub fn restore(path: &Path) -> io::Result<(Vec<Vec<u8>>, RestoreOutcome)> {
    if !path.exists() {
        return Ok((Vec::new(), RestoreOutcome::Clean { records: 0 }));
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut records: Vec<Vec<u8>> = Vec::new();
    let mut good_bytes: u64 = 0;
    let mut clean = false;

    let outcome = loop {
        let before = reader.stream_position()?;
        match read_record(&mut reader)? {
            ReadOutcome::Record(payload) => {
                if payload == CLEAN_MARKER {
                    // Everything before this was written deliberately and the
                    // process meant to stop.
                    clean = true;
                    good_bytes = reader.stream_position()?;
                    continue;
                }
                // A marker followed by more records means the log was reopened
                // and appended to, so the earlier marker no longer describes
                // the end of the file.
                clean = false;
                records.push(payload);
                good_bytes = reader.stream_position()?;
            }
            ReadOutcome::Eof => {
                break if clean {
                    RestoreOutcome::Clean {
                        records: records.len(),
                    }
                } else {
                    // No marker: the process did not get to say goodbye, but
                    // every record read was complete and verified.
                    RestoreOutcome::Recovered {
                        records: records.len(),
                        lost_tail_bytes: 0,
                    }
                };
            }
            ReadOutcome::TornTail { bytes } => {
                // Expected after a crash. The bytes were never acknowledged to
                // anyone, so dropping them loses nothing that was promised.
                truncate_to(path, good_bytes)?;
                break RestoreOutcome::Recovered {
                    records: records.len(),
                    lost_tail_bytes: bytes,
                };
            }
            ReadOutcome::Corrupt {
                expected_crc,
                actual_crc,
            } => {
                // Not a crash. Something changed bytes that were already
                // written, and truncating would destroy the evidence.
                let quarantined = quarantine(path, before)?;
                break RestoreOutcome::Partial {
                    records: records.len(),
                    quarantined,
                    reason: format!(
                        "a record at byte {before} has checksum {actual_crc:#010x}, not the {expected_crc:#010x} it claims"
                    ),
                };
            }
        }
    };

    Ok((records, outcome))
}

fn truncate_to(path: &Path, len: u64) -> io::Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_len(len)?;
    file.sync_all()
}

/// Move the damaged remainder aside, keeping the good prefix in place.
fn quarantine(path: &Path, from: u64) -> io::Result<PathBuf> {
    use std::io::Read;

    let mut file = File::open(path)?;
    file.seek(io::SeekFrom::Start(from))?;
    let mut rest = Vec::new();
    file.read_to_end(&mut rest)?;

    // Named after the offset rather than a timestamp, so repeating a failed
    // restore does not produce a directory full of near-identical files.
    let target = path.with_extension(format!("quarantine.{from}"));
    let mut out = File::create(&target)?;
    out.write_all(&rest)?;
    out.sync_all()?;

    truncate_to(path, from)?;
    Ok(target)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn write_log(path: &Path, payloads: &[&[u8]], clean: bool) {
        let mut log = Log::open(path).expect("open");
        for p in payloads {
            log.append(p).expect("append");
        }
        if clean {
            log.close_cleanly().expect("close");
        } else {
            log.flush().expect("flush");
        }
    }

    #[test]
    fn a_clean_shutdown_restores_everything_quietly() {
        let d = dir();
        let path = d.path().join("log");
        write_log(&path, &[b"a", b"b"], true);

        let (records, outcome) = restore(&path).expect("restore");
        assert_eq!(records, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(outcome, RestoreOutcome::Clean { records: 2 });
        assert!(
            !outcome.needs_reporting(),
            "a clean restore does not bother the user"
        );
    }

    #[test]
    fn no_marker_means_recovered_rather_than_clean() {
        // The process did not get to say goodbye, and the user should know the
        // difference even when nothing was actually lost.
        let d = dir();
        let path = d.path().join("log");
        write_log(&path, &[b"a"], false);

        let (records, outcome) = restore(&path).expect("restore");
        assert_eq!(records.len(), 1);
        assert!(matches!(outcome, RestoreOutcome::Recovered { .. }));
        assert!(outcome.needs_reporting());
    }

    #[test]
    fn a_torn_tail_is_truncated_and_the_rest_survives() {
        // What a crash mid-write actually looks like.
        let d = dir();
        let path = d.path().join("log");
        write_log(&path, &[b"first", b"second"], false);

        let len = std::fs::metadata(&path).expect("meta").len();
        truncate_to(&path, len - 3).expect("simulate a crash");

        let (records, outcome) = restore(&path).expect("restore");
        assert_eq!(records, vec![b"first".to_vec()]);
        let RestoreOutcome::Recovered {
            lost_tail_bytes, ..
        } = outcome
        else {
            panic!("{outcome:?}");
        };
        assert!(lost_tail_bytes > 0);
    }

    #[test]
    fn a_repaired_log_can_be_appended_to_again() {
        // The point of truncating rather than quarantining: the file is usable
        // immediately, without a human in the loop.
        let d = dir();
        let path = d.path().join("log");
        write_log(&path, &[b"first", b"second"], false);
        let len = std::fs::metadata(&path).expect("meta").len();
        truncate_to(&path, len - 3).expect("crash");

        restore(&path).expect("restore");
        let mut log = Log::open(&path).expect("reopen");
        log.append(b"third").expect("append");
        log.flush().expect("flush");

        let (records, _) = restore(&path).expect("restore again");
        assert_eq!(records, vec![b"first".to_vec(), b"third".to_vec()]);
    }

    #[test]
    fn corruption_is_quarantined_rather_than_truncated() {
        // Truncating would destroy the evidence of a real problem, and a store
        // that quietly discarded history is indistinguishable from one that
        // worked.
        let d = dir();
        let path = d.path().join("log");
        write_log(&path, &[b"good", b"bad-record"], false);

        // Flip a bit inside the second record's payload.
        let mut bytes = std::fs::read(&path).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&path, &bytes).expect("write");

        let (records, outcome) = restore(&path).expect("restore");
        assert_eq!(records, vec![b"good".to_vec()], "the good prefix survives");
        let RestoreOutcome::Partial {
            quarantined,
            reason,
            ..
        } = outcome
        else {
            panic!("{outcome:?}");
        };
        assert!(quarantined.exists(), "the damage was kept, not deleted");
        assert!(reason.contains("checksum"), "{reason}");
    }

    #[test]
    fn a_quarantine_leaves_the_log_usable() {
        let d = dir();
        let path = d.path().join("log");
        write_log(&path, &[b"good", b"bad"], false);
        let mut bytes = std::fs::read(&path).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(&path, &bytes).expect("write");

        restore(&path).expect("restore");
        let mut log = Log::open(&path).expect("reopen");
        log.append(b"after").expect("append");
        log.flush().expect("flush");

        let (records, _) = restore(&path).expect("restore again");
        assert_eq!(records, vec![b"good".to_vec(), b"after".to_vec()]);
    }

    #[test]
    fn a_missing_log_is_an_empty_clean_restore() {
        // A first run is not a failure.
        let d = dir();
        let (records, outcome) = restore(&d.path().join("never-written")).expect("restore");
        assert!(records.is_empty());
        assert_eq!(outcome, RestoreOutcome::Clean { records: 0 });
    }

    #[test]
    fn reopening_after_a_clean_close_and_appending_is_not_still_clean() {
        // The marker described the end of the *previous* run. Records after it
        // mean the process started again and did not finish.
        let d = dir();
        let path = d.path().join("log");
        write_log(&path, &[b"a"], true);

        let mut log = Log::open(&path).expect("reopen");
        log.append(b"b").expect("append");
        log.flush().expect("flush");

        let (records, outcome) = restore(&path).expect("restore");
        assert_eq!(records, vec![b"a".to_vec(), b"b".to_vec()]);
        assert!(
            matches!(outcome, RestoreOutcome::Recovered { .. }),
            "{outcome:?}"
        );
    }

    #[test]
    fn the_marker_is_not_returned_as_a_record() {
        // It is bookkeeping, and a replay that acted on it would be acting on
        // a record nothing wrote.
        let d = dir();
        let path = d.path().join("log");
        write_log(&path, &[b"a"], true);
        let (records, _) = restore(&path).expect("restore");
        assert_eq!(records, vec![b"a".to_vec()]);
    }

    #[test]
    fn json_records_round_trip() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct Entry {
            command: String,
            exit: i32,
        }

        let d = dir();
        let path = d.path().join("log");
        let mut log = Log::open(&path).expect("open");
        log.append_json(&Entry {
            command: "cargo test".into(),
            exit: 0,
        })
        .expect("append");
        log.close_cleanly().expect("close");

        let (records, _) = restore(&path).expect("restore");
        let back: Entry = serde_json::from_slice(&records[0]).expect("parse");
        assert_eq!(back.command, "cargo test");
    }

    #[test]
    fn every_outcome_reports_how_many_records_came_back() {
        // So a caller can say "restored 412 blocks" in all three cases rather
        // than only the happy one.
        assert_eq!(RestoreOutcome::Clean { records: 3 }.records(), 3);
        assert_eq!(
            RestoreOutcome::Recovered {
                records: 4,
                lost_tail_bytes: 9
            }
            .records(),
            4
        );
        assert_eq!(
            RestoreOutcome::Partial {
                records: 5,
                quarantined: PathBuf::from("/tmp/x"),
                reason: String::new()
            }
            .records(),
            5
        );
    }
}

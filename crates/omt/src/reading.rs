//! Reading an agent's own session file, from where the last read stopped.
//!
//! The file is JSON Lines and an agent appends to it. Three things about that
//! are easy to get wrong, and all three are silent:
//!
//! * **Re-reading it.** A long conversation's file reaches hundreds of
//!   megabytes. Reading the whole thing on every poll is a session that gets
//!   slower the longer somebody works, which is exactly backwards.
//! * **A line that has not finished arriving.** A record is appended with more
//!   than one write often enough to be caught mid-line. Parsing it drops the
//!   record; skipping it drops the record *permanently*, because the position
//!   has already moved past it. It has to be held.
//! * **A file that got shorter.** That is a different file — replaced, or a
//!   new conversation in the same place — not a rewound one. Seeking to the
//!   old position would read past its end forever.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The largest fragment held while waiting for a newline.
///
/// A line longer than this is not a record being written in pieces, it is a
/// file that is not what omt thinks it is. Dropping the fragment at a bound
/// keeps a wrong path from growing memory without limit.
const MAX_PARTIAL: usize = 1 << 22; // 4 MiB

/// How much is read from one file in one poll.
///
/// The first read of an hour-old conversation would otherwise parse the whole
/// file inside the frame loop. Read a bounded amount and come back: the next
/// poll is milliseconds away, and the position makes that free.
const MAX_PER_READ: usize = 1 << 20; // 1 MiB

/// Where reading of one file has got to.
#[derive(Debug, Default)]
pub struct Reading {
    pos: u64,
    partial: String,
}

impl Reading {
    /// Read whatever whole lines have appeared since the last call.
    ///
    /// Returns the records that parsed. Lines that do not parse are skipped:
    /// a session file is full of bookkeeping omt has no opinion on, so "not
    /// something I understand" is the ordinary case rather than an error.
    pub fn read(&mut self, path: &Path) -> Vec<serde_json::Value> {
        let Ok(mut file) = std::fs::File::open(path) else {
            // No file yet, or not readable. Nothing to say and nothing to
            // reset: the path was reported by the agent, and a file it has not
            // written yet is a file that will appear.
            return Vec::new();
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.pos {
            // Shorter than where reading had got to: a different file.
            self.pos = 0;
            self.partial.clear();
        }
        if len == self.pos {
            return Vec::new();
        }
        if file.seek(SeekFrom::Start(self.pos)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if file
            .take(MAX_PER_READ as u64)
            .read_to_end(&mut buf)
            .is_err()
        {
            return Vec::new();
        }
        // Advance by what was *read*, not by what parsed. A record that failed
        // to parse has still gone past, and re-reading it would return it
        // again on every poll for the rest of the session.
        self.pos += buf.len() as u64;

        // Lossy on purpose: a multi-byte character split across the boundary
        // of a bounded read would otherwise fail the whole chunk. It lands in
        // one line, and one unparsable line is skipped.
        self.partial.push_str(&String::from_utf8_lossy(&buf));
        let mut records = Vec::new();
        // The tail after the last newline is what has not finished arriving.
        // `split` gives it as the final element, which is empty exactly when
        // the read ended on a line boundary.
        let mut lines: Vec<&str> = self.partial.split('\n').collect();
        let tail = lines.pop().unwrap_or_default().to_owned();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str(line) {
                records.push(value);
            }
        }
        self.partial = if tail.len() > MAX_PARTIAL {
            String::new()
        } else {
            tail
        };
        records
    }
}

/// Where reading has got to, per session.
#[derive(Debug, Default)]
pub struct Readings(std::collections::BTreeMap<omt_types::SessionId, Reading>);

impl Readings {
    /// The reader for one session.
    pub fn entry(&mut self, session: omt_types::SessionId) -> &mut Reading {
        self.0.entry(session).or_default()
    }

    /// Forget a session's position.
    pub fn forget(&mut self, session: omt_types::SessionId) {
        self.0.remove(&session);
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
    use std::io::Write;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("omt-reading-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory");
        dir.join(name)
    }

    fn append(path: &Path, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("a file");
        file.write_all(text.as_bytes()).expect("a write");
    }

    #[test]
    fn only_what_is_new_is_read() {
        // The whole reason for a position: a conversation's file reaches
        // hundreds of megabytes, and re-reading it on every poll makes a
        // session slower the longer somebody works in it.
        let path = temp("new.jsonl");
        let _ = std::fs::remove_file(&path);
        append(&path, "{\"a\":1}\n");
        let mut reading = Reading::default();
        assert_eq!(reading.read(&path).len(), 1);
        assert!(reading.read(&path).is_empty());
        append(&path, "{\"a\":2}\n");
        let more = reading.read(&path);
        assert_eq!(more.len(), 1);
        assert_eq!(more[0]["a"], 2);
    }

    #[test]
    fn a_record_written_in_two_pieces_is_read_once_and_whole() {
        // Caught mid-line is normal, not exceptional. Parsing the fragment
        // loses the record; skipping it loses it forever, because the position
        // has moved past it.
        let path = temp("torn.jsonl");
        let _ = std::fs::remove_file(&path);
        append(&path, "{\"tool\":\"Ba");
        let mut reading = Reading::default();
        assert!(
            reading.read(&path).is_empty(),
            "a half line is not a record"
        );
        append(&path, "sh\"}\n");
        let records = reading.read(&path);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["tool"], "Bash");
    }

    #[test]
    fn a_file_that_got_shorter_is_a_different_file() {
        // Replaced, or a new conversation in the same place. Seeking to the
        // old position would read past the end of the new file forever.
        let path = temp("replaced.jsonl");
        let _ = std::fs::remove_file(&path);
        append(&path, "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n");
        let mut reading = Reading::default();
        assert_eq!(reading.read(&path).len(), 3);
        std::fs::write(&path, "{\"b\":1}\n").expect("a rewrite");
        let records = reading.read(&path);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["b"], 1);
    }

    #[test]
    fn a_line_that_is_not_a_record_does_not_stop_the_rest() {
        // A session file is mostly bookkeeping omt has no opinion on, so
        // "cannot read this one" must never mean "stop reading".
        let path = temp("junk.jsonl");
        let _ = std::fs::remove_file(&path);
        append(&path, "not json\n{\"a\":1}\n\n{\"a\":2}\n");
        let mut reading = Reading::default();
        assert_eq!(reading.read(&path).len(), 2);
    }

    #[test]
    fn a_file_that_is_not_there_says_nothing_and_keeps_waiting() {
        // The path came from the agent. A file it has not written yet is a
        // file that will appear, so this is not an error and not a reset.
        let path = temp("absent.jsonl");
        let _ = std::fs::remove_file(&path);
        let mut reading = Reading::default();
        assert!(reading.read(&path).is_empty());
        append(&path, "{\"a\":1}\n");
        assert_eq!(reading.read(&path).len(), 1);
    }
}

//! Reading a server-sent event stream.
//!
//! opencode's `serve` reports over SSE, and so do several other agents. The
//! format is small and its edges are where implementations go wrong: an event
//! is not complete until a blank line, `data:` may appear more than once and
//! the pieces are joined with newlines, and a comment line exists purely to
//! keep the connection alive and must not be mistaken for an event.
//!
//! Getting any of those wrong produces half an event or a phantom one, and
//! either becomes a card somebody answers.

/// One complete event off the stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Event {
    /// Its type, where the stream named one.
    pub name: Option<String>,
    /// The data, with multi-line payloads joined by newlines.
    pub data: String,
    /// The last id seen, which is what a reconnect resumes from.
    pub id: Option<String>,
}

/// Accumulates lines into events.
#[derive(Debug, Default)]
pub struct Decoder {
    current: Event,
    /// The id most recently seen, kept across events because a stream sends it
    /// once and expects a client to remember it — losing it means a reconnect
    /// replays from the beginning.
    last_id: Option<String>,
}

impl Decoder {
    /// A decoder with nothing buffered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The id a reconnect should resume from.
    #[must_use]
    pub fn last_id(&self) -> Option<&str> {
        self.last_id.as_deref()
    }

    /// Feed one line. Returns an event when the line completed one.
    pub fn line(&mut self, line: &str) -> Option<Event> {
        // A blank line ends the event. An empty one — no data at all — is a
        // keep-alive boundary and produces nothing, or an idle stream would
        // emit a phantom event every fifteen seconds.
        if line.is_empty() {
            if self.current.data.is_empty() && self.current.name.is_none() {
                return None;
            }
            let mut done = std::mem::take(&mut self.current);
            done.id = self.last_id.clone();
            return Some(done);
        }
        // A comment. Its whole purpose is to keep a connection open through a
        // proxy, and treating it as data would put `:ping` in a transcript.
        if line.starts_with(':') {
            return None;
        }

        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            // A field with no colon is a field with an empty value, per the
            // format — not a malformed line to discard.
            None => (line, ""),
        };
        match field {
            "event" => self.current.name = Some(value.to_owned()),
            "data" => {
                if !self.current.data.is_empty() {
                    self.current.data.push('\n');
                }
                self.current.data.push_str(value);
            }
            "id" => self.last_id = Some(value.to_owned()),
            // `retry` and anything unknown are ignored rather than erroring:
            // a stream is allowed to send fields a client does not know.
            _ => {}
        }
        None
    }

    /// Feed a whole chunk, returning every event it completed.
    ///
    /// A chunk that ends mid-event leaves the remainder buffered, which is the
    /// case that matters: a network hands over arbitrary slices, and an event
    /// split across two of them must not become two half events.
    pub fn feed(&mut self, chunk: &str) -> Vec<Event> {
        chunk
            .split('\n')
            .filter_map(|l| self.line(l.trim_end_matches('\r')))
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn an_event_is_not_complete_until_the_blank_line() {
        // Emitting on the data line would deliver a card before its options
        // arrived.
        let mut d = Decoder::new();
        assert!(d.line("event: message").is_none());
        assert!(d.line("data: hello").is_none());
        let event = d.line("").expect("the blank line completes it");
        assert_eq!(event.name.as_deref(), Some("message"));
        assert_eq!(event.data, "hello");
    }

    #[test]
    fn several_data_lines_join_with_newlines() {
        // The format says so, and a client that concatenated them would turn a
        // two-line message into one run-on line.
        let mut d = Decoder::new();
        d.line("data: first");
        d.line("data: second");
        assert_eq!(d.line("").expect("event").data, "first\nsecond");
    }

    #[test]
    fn a_comment_is_not_an_event() {
        // Its whole purpose is keeping a connection open through a proxy.
        let mut d = Decoder::new();
        assert!(d.line(": ping").is_none());
        assert!(d.line("").is_none(), "a keep-alive produced an event");
    }

    #[test]
    fn an_idle_stream_produces_nothing_rather_than_empty_events() {
        let mut d = Decoder::new();
        for _ in 0..5 {
            assert!(d.line("").is_none());
        }
    }

    #[test]
    fn an_event_split_across_chunks_arrives_once_and_whole() {
        // A network hands over arbitrary slices. Half an event delivered twice
        // is the bug this exists to prevent.
        let mut d = Decoder::new();
        assert!(d.feed("event: mes").is_empty());
        assert!(d.feed("sage\ndata: hi").is_empty());
        let events = d.feed("\n\n");
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].data, "hi");
    }

    #[test]
    fn the_id_is_remembered_so_a_reconnect_can_resume() {
        // A stream sends it once and expects the client to keep it; losing it
        // means replaying from the beginning after every hiccup.
        let mut d = Decoder::new();
        d.feed("id: 42\ndata: one\n\n");
        d.feed("data: two\n\n");
        assert_eq!(d.last_id(), Some("42"));
    }

    #[test]
    fn a_bare_data_field_dispatches_nothing() {
        // The spec's own rule: an empty data buffer dispatches no event. It is
        // also the only reading that keeps a keep-alive from becoming a phantom
        // message, which is why the two cases share a branch here.
        let mut d = Decoder::new();
        d.line("data");
        assert!(d.line("").is_none());
    }

    #[test]
    fn a_named_event_with_empty_data_still_arrives() {
        // The name is the content in this case — "the agent went idle" needs no
        // payload, and dropping it would lose a state change.
        let mut d = Decoder::new();
        d.line("event: idle");
        let event = d.line("").expect("a named event was dropped");
        assert_eq!(event.name.as_deref(), Some("idle"));
        assert_eq!(event.data, "");
    }

    #[test]
    fn carriage_returns_do_not_end_up_in_the_data() {
        // A stream from a server that writes CRLF would otherwise put a stray
        // \\r at the end of every message.
        let mut d = Decoder::new();
        let events = d.feed("data: hello\r\n\r\n");
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn a_field_omt_does_not_know_is_ignored_rather_than_failing() {
        // A stream is allowed to send fields a client has never heard of.
        let mut d = Decoder::new();
        d.line("retry: 3000");
        d.line("data: still here");
        assert_eq!(d.line("").expect("event").data, "still here");
    }
}

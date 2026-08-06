//! Speech to text.
//!
//! The design point is that a transcript arrives *progressively and revises
//! itself*. A provider emits partials that later change — "right" becomes
//! "write" once the next word arrives — so a surface that appended partials
//! would show text the user never said and then leave it there. Every partial
//! replaces the last; only a final is committed.

use serde::{Deserialize, Serialize};

/// A piece of transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    /// The text so far.
    pub text: String,
    /// Whether this replaces the previous partial or commits.
    ///
    /// The load-bearing field. A surface that appended every partial would
    /// accumulate a sentence the user never said.
    pub is_final: bool,
    /// The provider's confidence, where it reports one.
    pub confidence: Option<f32>,
}

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SttError {
    /// No provider is configured.
    #[error("no speech provider is configured")]
    NoProvider,
    /// The provider refused the credentials.
    #[error("the speech provider rejected its credentials")]
    Unauthorized,
    /// The provider failed.
    #[error("speech provider: {0}")]
    Provider(String),
    /// The audio was not something the provider accepts.
    #[error("unsupported audio: {0}")]
    UnsupportedAudio(String),
}

/// How audio is being supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    /// Samples per second.
    pub sample_rate: u32,
    /// How many channels.
    pub channels: u8,
}

impl AudioFormat {
    /// The format every provider accepts.
    pub const STANDARD: Self = Self {
        sample_rate: 16_000,
        channels: 1,
    };

    /// Whether a provider is likely to accept this without resampling.
    #[must_use]
    pub const fn is_standard(self) -> bool {
        self.sample_rate == 16_000 && self.channels == 1
    }
}

/// A speech provider.
///
/// A trait rather than one implementation, because the honest answer to "which
/// provider" is "whichever the user is already paying for". A build that hard
/// coded one would be a build most people cannot use.
pub trait SpeechProvider: Send + Sync {
    /// What to call it in a settings UI.
    fn name(&self) -> &str;

    /// Whether it is usable right now.
    ///
    /// Separate from transcribing so a settings screen can say "not configured"
    /// before the user records something and loses it.
    fn is_available(&self) -> bool;

    /// Transcribe a complete recording.
    ///
    /// # Errors
    /// Fails if the provider is unavailable, refuses the audio, or errors.
    fn transcribe(&self, audio: &[u8], format: AudioFormat) -> Result<Transcript, SttError>;
}

/// Accumulates partials into something a surface can render.
///
/// Holds exactly one uncommitted partial. That is the whole state machine, and
/// getting it wrong is what produces transcripts full of repeated half-words.
#[derive(Debug, Default)]
pub struct TranscriptBuffer {
    committed: String,
    partial: String,
}

impl TranscriptBuffer {
    /// An empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a transcript fragment.
    pub fn apply(&mut self, t: &Transcript) {
        if t.is_final {
            if !self.committed.is_empty() && !t.text.is_empty() {
                self.committed.push(' ');
            }
            self.committed.push_str(&t.text);
            self.partial.clear();
        } else {
            // Replaces rather than appends: the provider is revising its guess,
            // not continuing it.
            self.partial = t.text.clone();
        }
    }

    /// What to show right now, committed text plus the live partial.
    #[must_use]
    pub fn display(&self) -> String {
        match (self.committed.is_empty(), self.partial.is_empty()) {
            (true, _) => self.partial.clone(),
            (false, true) => self.committed.clone(),
            (false, false) => format!("{} {}", self.committed, self.partial),
        }
    }

    /// Only what has been committed.
    ///
    /// What is actually sent when the user hits enter: a partial is a guess the
    /// provider has not stood behind, and sending one puts words in their mouth.
    #[must_use]
    pub fn committed(&self) -> &str {
        &self.committed
    }

    /// Whether anything has been said.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.committed.is_empty() && self.partial.is_empty()
    }

    /// Start over.
    pub fn clear(&mut self) {
        self.committed.clear();
        self.partial.clear();
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

    fn partial(text: &str) -> Transcript {
        Transcript {
            text: text.to_owned(),
            is_final: false,
            confidence: None,
        }
    }

    fn final_(text: &str) -> Transcript {
        Transcript {
            text: text.to_owned(),
            is_final: true,
            confidence: Some(0.9),
        }
    }

    #[test]
    fn a_partial_replaces_the_one_before_it() {
        // The failure this prevents: appending every partial accumulates a
        // sentence the user never said.
        let mut b = TranscriptBuffer::new();
        b.apply(&partial("write"));
        b.apply(&partial("write the"));
        b.apply(&partial("write the code"));
        assert_eq!(b.display(), "write the code");
    }

    #[test]
    fn a_revised_partial_does_not_leave_the_old_guess_behind() {
        // "right" becomes "write" once the next word arrives.
        let mut b = TranscriptBuffer::new();
        b.apply(&partial("right"));
        b.apply(&partial("write the"));
        assert_eq!(b.display(), "write the");
    }

    #[test]
    fn a_final_commits_and_clears_the_partial() {
        let mut b = TranscriptBuffer::new();
        b.apply(&partial("hello wor"));
        b.apply(&final_("hello world"));
        assert_eq!(b.committed(), "hello world");
        assert_eq!(b.display(), "hello world");
    }

    #[test]
    fn finals_accumulate_across_utterances() {
        let mut b = TranscriptBuffer::new();
        b.apply(&final_("first sentence."));
        b.apply(&final_("second sentence."));
        assert_eq!(b.committed(), "first sentence. second sentence.");
    }

    #[test]
    fn a_partial_after_a_final_shows_but_is_not_committed() {
        // What the user is mid-way through saying is visible but not yet theirs
        // to send.
        let mut b = TranscriptBuffer::new();
        b.apply(&final_("run the"));
        b.apply(&partial("tes"));
        assert_eq!(b.display(), "run the tes");
        assert_eq!(b.committed(), "run the");
    }

    #[test]
    fn only_committed_text_is_what_gets_sent() {
        // A partial is a guess the provider has not stood behind; sending one
        // puts words in the user's mouth.
        let mut b = TranscriptBuffer::new();
        b.apply(&partial("delete everyth"));
        assert_eq!(b.committed(), "", "nothing to send yet");
    }

    #[test]
    fn an_empty_buffer_displays_nothing() {
        let b = TranscriptBuffer::new();
        assert!(b.is_empty());
        assert_eq!(b.display(), "");
    }

    #[test]
    fn clearing_discards_both_halves() {
        let mut b = TranscriptBuffer::new();
        b.apply(&final_("committed"));
        b.apply(&partial("pending"));
        b.clear();
        assert!(b.is_empty());
    }

    #[test]
    fn the_standard_format_is_what_providers_expect() {
        assert!(AudioFormat::STANDARD.is_standard());
        assert!(
            !AudioFormat {
                sample_rate: 44_100,
                channels: 2
            }
            .is_standard(),
            "so a caller knows to resample rather than being rejected later"
        );
    }

    #[test]
    fn a_provider_can_be_written_from_outside_this_crate() {
        // The honest answer to "which provider" is "whichever the user already
        // pays for", so this has to be implementable elsewhere.
        struct Mock;
        impl SpeechProvider for Mock {
            fn name(&self) -> &str {
                "mock"
            }
            fn is_available(&self) -> bool {
                true
            }
            fn transcribe(
                &self,
                _audio: &[u8],
                format: AudioFormat,
            ) -> Result<Transcript, SttError> {
                if !format.is_standard() {
                    return Err(SttError::UnsupportedAudio(format!("{format:?}")));
                }
                Ok(final_("transcribed"))
            }
        }

        let p = Mock;
        assert!(p.is_available());
        let t = p.transcribe(b"audio", AudioFormat::STANDARD).expect("ok");
        assert!(t.is_final);
        assert!(
            p.transcribe(
                b"audio",
                AudioFormat {
                    sample_rate: 8_000,
                    channels: 1
                }
            )
            .is_err()
        );
    }

    #[test]
    fn availability_is_checkable_before_anything_is_recorded() {
        // So a settings screen says "not configured" rather than the user
        // recording something and losing it.
        struct Unconfigured;
        impl SpeechProvider for Unconfigured {
            fn name(&self) -> &str {
                "unconfigured"
            }
            fn is_available(&self) -> bool {
                false
            }
            fn transcribe(&self, _: &[u8], _: AudioFormat) -> Result<Transcript, SttError> {
                Err(SttError::NoProvider)
            }
        }
        assert!(!Unconfigured.is_available());
    }
}

/// A speech-to-text engine omt can drive.
///
/// A trait rather than a match on a provider name, because the interesting
/// providers are the ones omt has not heard of: a local whisper.cpp, a
/// company's own endpoint, something that does not exist yet. Adding one must
/// not mean editing this crate.
///
/// omt itself never ships a key and never calls a provider without one. That is
/// the whole of BYOK: the user's audio goes where the user said, paid for by
/// the user's own account, and omt is not in the middle of it.
pub trait SttProvider: Send + Sync {
    /// Its id, which is what configuration names.
    fn id(&self) -> &str;

    /// What it is called in a settings screen.
    fn label(&self) -> &str;

    /// Whether it needs a credential.
    ///
    /// A local engine does not, and a settings screen that demanded a key for
    /// whisper.cpp would make the private option look like the hard one.
    fn needs_key(&self) -> bool {
        true
    }

    /// Which audio it accepts.
    fn accepts(&self) -> AudioFormat;

    /// Turn a chunk of audio into a transcript.
    ///
    /// # Errors
    /// Whatever the engine could not do — a missing key, a refused request, an
    /// audio format it does not take.
    fn transcribe(&self, audio: &[u8]) -> Result<Transcript, SttError>;
}

/// Every provider an instance knows.
///
/// A registry rather than an enum, for the same reason the adapter set is one:
/// the extension point is the registry, and the names are only labels.
#[derive(Default)]
pub struct ProviderSet {
    providers: Vec<Box<dyn SttProvider>>,
}

impl ProviderSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a provider, replacing any with the same id.
    pub fn insert(&mut self, provider: Box<dyn SttProvider>) {
        self.providers.retain(|p| p.id() != provider.id());
        self.providers.push(provider);
    }

    /// One provider by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&dyn SttProvider> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(std::convert::AsRef::as_ref)
    }

    /// Every provider.
    #[must_use]
    pub fn all(&self) -> Vec<&dyn SttProvider> {
        self.providers
            .iter()
            .map(std::convert::AsRef::as_ref)
            .collect()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod provider_tests {
    use super::*;

    /// A provider written using only the public surface. If this stops
    /// compiling, the extension point has closed.
    struct Local;

    impl SttProvider for Local {
        fn id(&self) -> &str {
            "whisper-local"
        }
        fn label(&self) -> &str {
            "whisper.cpp"
        }
        fn needs_key(&self) -> bool {
            false
        }
        fn accepts(&self) -> AudioFormat {
            AudioFormat::STANDARD
        }
        fn transcribe(&self, _audio: &[u8]) -> Result<Transcript, SttError> {
            Ok(Transcript {
                text: "hello".to_owned(),
                is_final: true,
                confidence: None,
            })
        }
    }

    #[test]
    fn a_provider_can_be_written_from_outside_this_crate() {
        let mut set = ProviderSet::new();
        set.insert(Box::new(Local));
        let p = set.get("whisper-local").expect("registered");
        assert_eq!(p.label(), "whisper.cpp");
    }

    #[test]
    fn a_local_engine_is_not_made_to_pretend_it_needs_a_key() {
        // A settings screen that demanded one for whisper.cpp would make the
        // private option look like the hard one.
        assert!(!Local.needs_key());
    }

    #[test]
    fn a_second_provider_with_the_same_id_replaces_the_first() {
        // Overriding a built-in with a better one must not need a fork.
        let mut set = ProviderSet::new();
        set.insert(Box::new(Local));
        set.insert(Box::new(Local));
        assert_eq!(set.all().len(), 1);
    }

    #[test]
    fn an_instance_with_no_providers_says_so_rather_than_pretending() {
        // Which is omt's shipped state: no key, no provider, no audio leaving
        // the machine until the user says where it goes.
        assert!(ProviderSet::new().is_empty());
    }
}

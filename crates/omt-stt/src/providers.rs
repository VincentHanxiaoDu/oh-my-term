//! The engines the architecture names: Deepgram, OpenAI, and a local one.
//!
//! Each takes the user's own key and sends the user's audio to the endpoint the
//! user chose. omt ships no key and holds none — that is what BYOK means, and
//! it is why the key arrives as a parameter rather than being read from a file
//! somewhere in here.
//!
//! The endpoint is configurable on every one of them. Not for flexibility's
//! sake: a company with its own Deepgram-compatible endpoint, or somebody
//! running whisper.cpp behind an OpenAI-shaped API, should not have to fork this
//! to point at it.

use crate::{AudioFormat, SttError, SttProvider, Transcript};

/// How long to wait on a transcription before giving up.
///
/// Dictation is interactive: a request that has taken this long has already
/// lost the user, and holding the connection open makes the next attempt queue
/// behind it.
const TIMEOUT_SECS: u64 = 20;

/// Deepgram's streaming-shaped REST endpoint.
pub struct Deepgram {
    key: String,
    endpoint: String,
}

impl Deepgram {
    /// With the user's key.
    #[must_use]
    pub fn new(key: String) -> Self {
        Self {
            key,
            endpoint: "https://api.deepgram.com/v1/listen".to_owned(),
        }
    }

    /// Point it somewhere else.
    #[must_use]
    pub fn at(mut self, endpoint: String) -> Self {
        self.endpoint = endpoint;
        self
    }
}

impl SttProvider for Deepgram {
    fn id(&self) -> &str {
        "deepgram"
    }

    fn label(&self) -> &str {
        "Deepgram"
    }

    fn accepts(&self) -> AudioFormat {
        AudioFormat::STANDARD
    }

    fn transcribe(&self, audio: &[u8]) -> Result<Transcript, SttError> {
        // Refused before the request rather than after: an empty body comes
        // back as a confusing 400, and the user is left thinking their key is
        // wrong when the microphone simply produced nothing.
        if audio.is_empty() {
            return Err(SttError::UnsupportedAudio(
                "there was no audio to send".to_owned(),
            ));
        }
        // The config block comes first. Setting headers before it drops them:
        // `.config()` returns a different builder and `.build()` hands back a
        // fresh request, so anything set earlier is gone — silently, which is
        // how a request goes out with no Authorization header at all.
        let body: serde_json::Value = ureq::post(&self.endpoint)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(TIMEOUT_SECS)))
            .build()
            .header("Authorization", &format!("Token {}", self.key))
            .header("Content-Type", "audio/wav")
            .send(audio)
            .map_err(|e| SttError::Provider(e.to_string()))?
            .body_mut()
            .read_json()
            .map_err(|e| SttError::Provider(e.to_string()))?;
        parse_deepgram(&body)
    }
}

/// Pull the transcript out of a Deepgram response.
///
/// Separated so the shape can be tested without a network: what a response
/// means is a pure function of the response.
pub fn parse_deepgram(body: &serde_json::Value) -> Result<Transcript, SttError> {
    let alt = body
        .pointer("/results/channels/0/alternatives/0")
        .ok_or_else(|| SttError::Provider("the response had no transcript in it".to_owned()))?;
    Ok(Transcript {
        text: alt
            .get("transcript")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_owned(),
        // A REST response is the whole answer. Claiming it might be revised
        // would leave the client waiting for a correction that never comes.
        is_final: true,
        confidence: alt
            .get("confidence")
            .and_then(serde_json::Value::as_f64)
            .map(|c| c as f32),
    })
}

/// OpenAI's transcription endpoint, and anything that copies its shape.
pub struct OpenAi {
    key: String,
    endpoint: String,
    model: String,
}

impl OpenAi {
    /// With the user's key.
    #[must_use]
    pub fn new(key: String) -> Self {
        Self {
            key,
            endpoint: "https://api.openai.com/v1/audio/transcriptions".to_owned(),
            model: "whisper-1".to_owned(),
        }
    }

    /// Point it somewhere else — a local server speaking the same shape counts.
    #[must_use]
    pub fn at(mut self, endpoint: String) -> Self {
        self.endpoint = endpoint;
        self
    }

    /// Use a different model.
    #[must_use]
    pub fn model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

impl SttProvider for OpenAi {
    fn id(&self) -> &str {
        "openai"
    }

    fn label(&self) -> &str {
        "OpenAI"
    }

    fn accepts(&self) -> AudioFormat {
        AudioFormat::STANDARD
    }

    fn transcribe(&self, audio: &[u8]) -> Result<Transcript, SttError> {
        if audio.is_empty() {
            return Err(SttError::UnsupportedAudio(
                "there was no audio to send".to_owned(),
            ));
        }
        let body = multipart_body(audio, &self.model);
        let response: serde_json::Value = ureq::post(&self.endpoint)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(TIMEOUT_SECS)))
            .build()
            .header("Authorization", &format!("Bearer {}", self.key))
            .header(
                "Content-Type",
                &format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
            )
            .send(&body[..])
            .map_err(|e| SttError::Provider(e.to_string()))?
            .body_mut()
            .read_json()
            .map_err(|e| SttError::Provider(e.to_string()))?;
        parse_openai(&response)
    }
}

/// Pull the transcript out of an OpenAI-shaped response.
pub fn parse_openai(body: &serde_json::Value) -> Result<Transcript, SttError> {
    let text = body
        .get("text")
        .and_then(|t| t.as_str())
        // An error body has a message in it, and repeating that is far more
        // use than "the response had no transcript".
        .ok_or_else(|| {
            SttError::Provider(
                body.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("the response had no transcript in it")
                    .to_owned(),
            )
        })?;
    Ok(Transcript {
        text: text.to_owned(),
        is_final: true,
        confidence: None,
    })
}

/// The boundary for a multipart body.
const MULTIPART_BOUNDARY: &str = "omt-audio-boundary";

/// Build the multipart body an OpenAI-shaped endpoint expects.
///
/// Hand-rolled because the alternative is a multipart crate for two fields, and
/// tested for the same reason every hand-rolled encoder here is: the failure is
/// a request that is refused with a message about the wrong thing.
pub fn multipart_body(audio: &[u8], model: &str) -> Vec<u8> {
    let mut body = Vec::new();
    let mut part = |header: &str| body.extend_from_slice(header.as_bytes());
    part(&format!("--{MULTIPART_BOUNDARY}\r\n"));
    part("Content-Disposition: form-data; name=\"model\"\r\n\r\n");
    part(model);
    part(&format!("\r\n--{MULTIPART_BOUNDARY}\r\n"));
    part("Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n");
    part("Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(audio);
    body.extend_from_slice(format!("\r\n--{MULTIPART_BOUNDARY}--\r\n").as_bytes());
    body
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "in a test, expect() is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn a_deepgram_response_yields_its_transcript_and_confidence() {
        let body = serde_json::json!({
            "results": { "channels": [{ "alternatives": [{
                "transcript": "run the tests",
                "confidence": 0.97
            }]}]}
        });
        let t = parse_deepgram(&body).expect("parsed");
        assert_eq!(t.text, "run the tests");
        assert!(t.confidence.is_some_and(|c| c > 0.9));
    }

    #[test]
    fn a_rest_response_is_final_rather_than_maybe_revised() {
        // Claiming it might change would leave the client waiting for a
        // correction that never arrives, with the text stuck uncommitted.
        let body = serde_json::json!({
            "results": { "channels": [{ "alternatives": [{ "transcript": "x" }]}]}
        });
        assert!(parse_deepgram(&body).expect("parsed").is_final);
    }

    #[test]
    fn a_deepgram_response_with_no_transcript_is_an_error_not_empty_text() {
        // Empty text would be committed into somebody's command line as if the
        // engine had heard silence.
        assert!(parse_deepgram(&serde_json::json!({ "results": {} })).is_err());
    }

    #[test]
    fn an_openai_error_body_is_reported_with_its_own_message() {
        // "The response had no transcript" sends somebody to check their
        // microphone when the answer is that the key is wrong.
        let body = serde_json::json!({ "error": { "message": "Incorrect API key provided" }});
        let err = parse_openai(&body).expect_err("an error body parsed as success");
        assert!(err.to_string().contains("Incorrect API key"), "{err}");
    }

    #[test]
    fn an_openai_response_yields_its_text() {
        let body = serde_json::json!({ "text": "hello there" });
        assert_eq!(parse_openai(&body).expect("parsed").text, "hello there");
    }

    #[test]
    fn the_multipart_body_carries_both_the_model_and_the_audio() {
        let body = multipart_body(b"RIFFdata", "whisper-1");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"model\""), "{text}");
        assert!(text.contains("whisper-1"));
        assert!(text.contains("filename=\"audio.wav\""));
        assert!(
            body.windows(8).any(|w| w == b"RIFFdata"),
            "the audio was lost"
        );
    }

    #[test]
    fn the_multipart_body_is_closed_properly() {
        // An unterminated body is refused with a message about the wrong thing,
        // and somebody spends an afternoon on their key.
        let body = multipart_body(b"x", "m");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.ends_with(&format!("--{MULTIPART_BOUNDARY}--\r\n")),
            "{text}"
        );
    }

    #[test]
    fn empty_audio_is_refused_before_it_reaches_the_network() {
        // An empty body comes back as a confusing 400, and the user concludes
        // their key is wrong when the microphone simply produced nothing.
        let err = Deepgram::new("k".to_owned())
            .transcribe(&[])
            .expect_err("empty audio was sent");
        assert!(err.to_string().contains("no audio"), "{err}");
    }

    #[test]
    fn every_provider_can_be_pointed_somewhere_else() {
        // A company with its own endpoint, or whisper.cpp behind an
        // OpenAI-shaped API, must not have to fork this.
        let d = Deepgram::new("k".to_owned()).at("http://localhost:9000/listen".to_owned());
        assert_eq!(d.endpoint, "http://localhost:9000/listen");
        let o = OpenAi::new("k".to_owned()).at("http://localhost:8080/v1".to_owned());
        assert_eq!(o.endpoint, "http://localhost:8080/v1");
    }
}

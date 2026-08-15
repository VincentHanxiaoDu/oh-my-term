//! A noise, played by whatever the machine already has.
//!
//! omt ships no audio. A repository that carries media carries its licence,
//! its attribution and an argument about taste; every system this runs on
//! already has sounds somebody chose, and a person who wants a particular one
//! can name a file.
//!
//! Everything here fails silently. A missing player, a missing file, a machine
//! with no audio at all — none of that is worth a message on somebody's
//! screen, and none of it may cost the frame it was decided in, so the playing
//! happens on a thread of its own.

use std::path::{Path, PathBuf};

/// Which noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// Somebody is being waited for.
    NeedsYou,
    /// Something finished while nobody was watching.
    Finished,
}

/// Where the sounds come from.
///
/// Passed in rather than read from a global, because "which file" is a
/// configuration answer and this module is the one that must not know how
/// configuration is stored.
#[derive(Debug, Clone, Default)]
pub struct Sounds {
    /// What to play when somebody is being waited for.
    pub needs_you: Option<PathBuf>,
    /// What to play when something finished.
    pub finished: Option<PathBuf>,
    /// Whether to make any noise at all.
    pub muted: bool,
}

impl Sounds {
    /// The file for one sound, falling back to the system's own.
    #[must_use]
    pub fn file_for(&self, sound: Sound) -> Option<PathBuf> {
        let chosen = match sound {
            Sound::NeedsYou => self.needs_you.clone(),
            Sound::Finished => self.finished.clone(),
        };
        chosen.or_else(|| system_sound(sound))
    }
}

/// Play one, on a thread of its own.
///
/// Returns immediately. A frame loop that waited for an audio player would
/// stutter every time an agent finished, which is the opposite of the point.
pub fn play(sound: Sound, sounds: &Sounds) {
    if sounds.muted || std::env::var_os("OMT_NO_SOUND").is_some() {
        return;
    }
    let Some(file) = sounds.file_for(sound) else {
        return;
    };
    std::thread::spawn(move || {
        let _ = run_player(&file);
    });
}

/// A sound the operating system already ships.
///
/// Two different ones where the platform has them: "somebody is waiting for
/// you" and "it finished" are different messages, and one noise for both makes
/// the distinction the whole feature rests on inaudible.
#[must_use]
pub fn system_sound(sound: Sound) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let candidates: [&str; 2] = match sound {
        Sound::NeedsYou => [
            "/System/Library/Sounds/Ping.aiff",
            "/System/Library/Sounds/Tink.aiff",
        ],
        Sound::Finished => [
            "/System/Library/Sounds/Glass.aiff",
            "/System/Library/Sounds/Pop.aiff",
        ],
    };
    #[cfg(not(target_os = "macos"))]
    let candidates: [&str; 2] = match sound {
        Sound::NeedsYou => [
            "/usr/share/sounds/freedesktop/stereo/message.oga",
            "/usr/share/sounds/freedesktop/stereo/bell.oga",
        ],
        Sound::Finished => [
            "/usr/share/sounds/freedesktop/stereo/complete.oga",
            "/usr/share/sounds/freedesktop/stereo/bell.oga",
        ],
    };
    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// Hand the file to whatever this machine plays sound with.
fn run_player(file: &Path) -> std::io::Result<()> {
    // In order of how likely each is to exist and how little it prints. Each
    // is tried only if the one before was not found — a player that ran and
    // failed has answered, and trying the next would play the sound twice on
    // a machine where both work.
    let players: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("afplay", &[])]
    } else {
        &[
            ("paplay", &[]),
            ("pw-play", &[]),
            ("canberra-gtk-play", &["-f"]),
            ("aplay", &["-q"]),
        ]
    };
    for (program, args) in players {
        let mut command = std::process::Command::new(program);
        command.args(*args).arg(file);
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        match command.status() {
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
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
    fn a_named_file_wins_over_the_systems_own() {
        // Somebody who names a file has chosen; falling back over their choice
        // would make the setting look broken rather than unset.
        let sounds = Sounds {
            needs_you: Some(PathBuf::from("/tmp/mine.wav")),
            ..Sounds::default()
        };
        assert_eq!(
            sounds.file_for(Sound::NeedsYou),
            Some(PathBuf::from("/tmp/mine.wav"))
        );
    }

    #[test]
    fn being_waited_for_and_finishing_do_not_sound_the_same() {
        // One noise for both makes the distinction the whole feature rests on
        // inaudible. Skipped where the platform ships neither, which is not a
        // fault of omt's.
        let (Some(waiting), Some(done)) =
            (system_sound(Sound::NeedsYou), system_sound(Sound::Finished))
        else {
            return;
        };
        assert_ne!(waiting, done);
    }

    #[test]
    fn muted_plays_nothing_and_costs_nothing() {
        // Returns rather than spawning: the check is that this does not reach
        // a thread or a process at all.
        let sounds = Sounds {
            muted: true,
            ..Sounds::default()
        };
        play(Sound::NeedsYou, &sounds);
    }
}

//! Everything an `OSC` sequence can mean.
//!
//! Split from the emulator because it is the widest surface in it: titles,
//! working directories, hyperlinks, colours, the clipboard, shell integration,
//! notifications and an agent's own reports all arrive through the same
//! two-character introducer. The file it came from was over the length this
//! repository allows, which is a rule about how much a reader has to hold at
//! once.

use crate::action::{ClipboardSelection, ColorSlot, HostAction, HyperlinkEvent};
use crate::cell::Color;
use crate::terminal::{AGENT_EVENT_MARKER, State};
use crate::{BlockEvent, HyperlinkId};

impl State {
    /// Called from the parser's `Perform` implementation, which stays in the
    /// emulator: the trait belongs where the parser is driven, and only the
    /// body of this one arm moved.
    pub(crate) fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        let Some(&code) = params.first() else { return };
        let text = |i: usize| -> String {
            params
                .get(i)
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_default()
        };
        match code {
            b"0" | b"2" => {
                let t = text(1);
                self.title = Some(t.clone());
                self.emit(HostAction::SetTitle(t));
            }
            b"1" => self.emit(HostAction::SetIconName(text(1))),
            // What a long build in a pane nobody is watching does when it
            // finishes. omt is the terminal it is talking to.
            b"9" => {
                let body = text(1);
                if !body.is_empty() {
                    self.emit(HostAction::Notify { title: None, body });
                }
            }
            // `777 ; notify ; <title> ; <body>`, and the one variant that
            // matters more: a structured event an agent publishes about
            // itself, carried as JSON behind a marker. Recognized here so an
            // agent with no hook installed is still legible — at a weaker
            // tier, because anything in the pane can write these bytes.
            b"777" => {
                if text(1) != "notify" {
                    return;
                }
                let second = text(2);
                if second == AGENT_EVENT_MARKER {
                    let payload = params
                        .get(3..)
                        .map(|rest| {
                            rest.iter()
                                .map(|b| String::from_utf8_lossy(b).into_owned())
                                .collect::<Vec<_>>()
                                // Rejoined, because JSON contains semicolons
                                // and the parser split on every one of them.
                                .join(";")
                        })
                        .unwrap_or_default();
                    if !payload.is_empty() {
                        self.emit(HostAction::AgentEvent(payload));
                    }
                    return;
                }
                let body = text(3);
                if body.is_empty() && second.is_empty() {
                    return;
                }
                self.emit(if body.is_empty() {
                    HostAction::Notify {
                        title: None,
                        body: second,
                    }
                } else {
                    HostAction::Notify {
                        title: Some(second),
                        body,
                    }
                });
            }
            b"7" => self.emit(HostAction::SetCwd { url: text(1) }),
            b"8" => {
                // OSC 8 ; params ; uri
                let uri = text(2);
                if uri.is_empty() {
                    self.open_link = None;
                    self.emit(HostAction::Hyperlink(HyperlinkEvent::Closed));
                } else {
                    let id = HyperlinkId(u32::try_from(self.hyperlinks.len()).unwrap_or(0));
                    self.hyperlinks.push(uri.clone());
                    let g = self.grid();
                    self.open_link = Some((id, g.cursor.row, g.cursor.col));
                    let explicit = text(1);
                    self.emit(HostAction::Hyperlink(HyperlinkEvent::Opened {
                        id: explicit
                            .strip_prefix("id=")
                            .map(std::borrow::ToOwned::to_owned),
                        uri,
                    }));
                }
            }
            b"52" => {
                let selection = if text(1).starts_with('p') {
                    ClipboardSelection::Primary
                } else {
                    ClipboardSelection::Clipboard
                };
                let payload = text(2);
                if payload == "?" {
                    self.emit(HostAction::ClipboardRead { selection });
                } else {
                    // Decoded here, honoured by policy elsewhere: a program
                    // must not be able to set the clipboard unobserved.
                    let data = decode_base64(payload.as_bytes());
                    self.emit(HostAction::ClipboardWrite { selection, data });
                }
            }
            b"133" => {
                let event = match text(1).as_bytes().first() {
                    Some(b'A') => Some(BlockEvent::PromptStart),
                    Some(b'B') => Some(BlockEvent::CommandStart),
                    Some(b'C') => Some(BlockEvent::OutputStart),
                    Some(b'D') => Some(BlockEvent::CommandEnd {
                        exit_code: params
                            .get(2)
                            .and_then(|b| std::str::from_utf8(b).ok())
                            .and_then(|s| s.parse().ok()),
                    }),
                    _ => None,
                };
                if let Some(e) = event {
                    self.pending_blocks.push(e);
                    self.emit(HostAction::Block(e));
                }
            }
            b"1337" => {
                let body = text(1);
                if let Some(rest) = body.strip_prefix("SetUserVar=")
                    && let Some((key, value)) = rest.split_once('=')
                {
                    self.emit(HostAction::SetUserVar {
                        key: key.to_owned(),
                        value: String::from_utf8_lossy(&decode_base64(value.as_bytes()))
                            .into_owned(),
                    });
                }
            }
            b"4" => {
                if let Ok(i) = text(1).parse::<u8>() {
                    self.emit(HostAction::SetColor {
                        slot: ColorSlot::Indexed(i),
                        color: parse_x_color(&text(2)),
                    });
                }
            }
            b"10" | b"11" | b"12" => {
                let slot = match code {
                    b"10" => ColorSlot::Foreground,
                    b"11" => ColorSlot::Background,
                    _ => ColorSlot::Cursor,
                };
                // `?` asks; anything else sets. Programs ask this to decide
                // whether they are on a light or a dark terminal — an agent
                // CLI is one of the programs that does — and a terminal that
                // stays silent leaves them to guess, which is wrong about half
                // the time and looks like *their* bug.
                if text(1).trim() == "?" {
                    let known = match slot {
                        ColorSlot::Foreground => self.colours.0.clone(),
                        ColorSlot::Background => self.colours.1.clone(),
                        // The cursor's colour is omt's to draw and not
                        // something it has been told, so there is nothing
                        // honest to answer.
                        _ => None,
                    };
                    // Silence when omt does not know. Inventing a colour is
                    // worse: a program told the background is black on a white
                    // terminal renders unreadably, and has been told to.
                    if let Some(colour) = known {
                        let code = String::from_utf8_lossy(code).into_owned();
                        // Terminated with ST rather than BEL — both are
                        // accepted, and a bell that reaches a program
                        // expecting a string terminator is a beep somebody
                        // hears for no reason.
                        self.emit(HostAction::Reply(
                            format!("\x1b]{code};{colour}\x1b\\").into_bytes(),
                        ));
                    }
                    return;
                }
                self.emit(HostAction::SetColor {
                    slot,
                    color: parse_x_color(&text(1)),
                });
            }
            _ => {}
        }
    }
}

/// Decode the base64 an OSC 52 carries.
///
/// Written out rather than pulled in: it is twenty lines, and a dependency in
/// the terminal core is a dependency in everything.
pub(crate) fn decode_base64(input: &[u8]) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut acc = 0u32;
    let mut bits = 0u32;
    let mut out = Vec::new();
    for b in input {
        if *b == b'=' {
            break;
        }
        let Some(v) = TABLE.iter().position(|t| t == b) else {
            continue;
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    out
}

/// Parse the `rgb:RRRR/GGGG/BBBB` and `#RRGGBB` spellings X uses.
pub(crate) fn parse_x_color(s: &str) -> Option<Color> {
    if let Some(rest) = s.strip_prefix("rgb:") {
        let mut parts = rest.split('/');
        let mut take = || -> Option<u8> {
            let p = parts.next()?;
            let v = u32::from_str_radix(p, 16).ok()?;
            // Components may be 1 to 4 hex digits wide; scale to eight bits.
            let scaled = match p.len() {
                1 => v * 17,
                2 => v,
                3 => v >> 4,
                _ => v >> 8,
            };
            u8::try_from(scaled).ok()
        };
        return Some(Color::rgb(take()?, take()?, take()?));
    }
    if let Some(hex) = s.strip_prefix('#')
        && hex.len() == 6
    {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return Some(Color::rgb(
            ((v >> 16) & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            (v & 0xff) as u8,
        ));
    }
    None
}

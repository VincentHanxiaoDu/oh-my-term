//! One module per agent.

mod claude_code;
mod codex;
mod copilot;
mod cursor;
mod floor;
mod gemini;
mod generic_acp;

pub use claude_code::ClaudeCode;
pub use codex::Codex;
pub use copilot::Copilot;
pub use cursor::Cursor;
pub use floor::{HeuristicFloor, ScreenSignals, guess_activity};
pub use gemini::Gemini;
pub use generic_acp::{GenericAcp, turn_from_stop_reason, turn_start};

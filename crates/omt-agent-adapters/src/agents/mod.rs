//! One module per agent.

mod claude_code;
mod floor;
mod generic_acp;

pub use claude_code::ClaudeCode;
pub use floor::{HeuristicFloor, ScreenSignals, guess_activity};
pub use generic_acp::{GenericAcp, turn_from_stop_reason, turn_start};

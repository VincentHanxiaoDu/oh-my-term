//! The hook binary an agent executes.
//!
//! Installed into each agent's own hook configuration. It reads the agent's
//! payload on stdin, reports it to the local instance, writes the document the
//! agent expects on stdout, and exits zero — **always** zero.
//!
//! It is a separate binary from `omt` so that it starts in single-digit
//! milliseconds. An agent runs this on every hook event, several per tool call;
//! anything linked here is startup cost paid by somebody else's agent.

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use omt_hook::{EXIT_OK, HookEnv, drain_stdin, emit_proceed, parse_agent, read_payload, report};

fn main() -> ExitCode {
    // Nothing below may propagate a failure. Every path ends in the same place:
    // print what the agent expects, exit zero. An observation is worth having;
    // it is not worth an agent hanging for.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let agent = parse_agent(&args);

    let result = std::panic::catch_unwind(|| {
        let env = HookEnv::from_process(agent);
        let interactive = io::stdin().is_terminal();

        if !env.is_worth_trying() {
            // omt did not spawn this agent. Nothing to report to, and trying
            // anyway would cost a syscall on every hook event forever.
            //
            // But stdin is still drained first. The agent is mid-write to this
            // pipe, and exiting without reading hands it a broken pipe part way
            // through — which it reports as the hook exiting 127, and then it
            // dies. The guard clause that skips the work must not skip the
            // read.
            if !interactive {
                drain_stdin(io::stdin().lock());
            }
            return;
        }

        let payload = if interactive {
            None
        } else {
            read_payload(io::stdin().lock())
        };
        let _ = report(&env, payload.as_ref());
    });

    if result.is_err() {
        // A panic here is a bug in omt, and it is still not the agent's
        // problem. Swallowed rather than propagated, and the agent proceeds.
    }

    let _ = emit_proceed(io::stdout().lock(), agent);
    ExitCode::from(EXIT_OK as u8)
}

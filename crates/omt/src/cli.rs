//! `omt <group> <verb>` — the CLI derived from the catalog.
//!
//! `docs/architecture/03-capability-catalog.md` §1, artifact 5: the subcommand
//! tree is one of the seven things derived from the single declaration, so that
//! nothing is hand-written twice and nothing can drift. A capability that
//! exists is reachable from a shell the moment it is declared, with no
//! per-capability argument parsing to forget to write.
//!
//! It also carries 22 §8.1's contract, which is what makes it usable from a
//! script rather than only by a person:
//!
//! - **`--json` on everything**, and the schema is the capability's own.
//! - **stdout is data, stderr is diagnostics.** So `omt session list --json |
//!   jq` is always safe, whatever omt has to say about it.
//! - **No TTY required, ever.** Anything that would prompt exits 3 naming the
//!   flag that would have avoided it.
//! - **Documented exit codes**, because a script's error handling is only as
//!   good as the codes it can branch on.
//!
//! Arguments are `--key value`, and their names are the capability's own input
//! field names. There is no per-capability flag table to drift from the schema,
//! which is the same reason the tree itself is derived.

use crate::automation::Exit;

/// What a parsed command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The capability name, `group.verb`.
    pub capability: String,
    /// The arguments, as they will be sent.
    pub input: std::collections::BTreeMap<String, serde_json::Value>,
    /// Whether the caller wants machine-readable output.
    pub json: bool,
}

/// Why a command line could not be turned into a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Not enough words to name a capability.
    NotACapability,
    /// A flag with no value after it.
    DanglingFlag {
        /// Which flag.
        flag: String,
    },
    /// A bare word where a flag was expected.
    Unexpected {
        /// What was found.
        word: String,
    },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotACapability => write!(f, "name a group and a verb, as in `omt session list`"),
            Self::DanglingFlag { flag } => write!(f, "`{flag}` needs a value after it"),
            Self::Unexpected { word } => {
                write!(f, "`{word}` is not a flag; arguments are given as --name value")
            }
        }
    }
}

/// Read a value the way a caller meant it.
///
/// A number stays a number and `true` stays a boolean, because a schema that
/// wants an integer will not take `"3"`. Anything else is a string, including
/// something that merely looks like JSON — quoting a path that begins with `{`
/// should not turn it into an object.
fn value(text: &str) -> serde_json::Value {
    match text {
        "true" => return serde_json::Value::Bool(true),
        "false" => return serde_json::Value::Bool(false),
        _ => {}
    }
    if let Ok(n) = text.parse::<i64>() {
        return serde_json::Value::from(n);
    }
    serde_json::Value::String(text.to_owned())
}

/// Turn a command line into a call.
///
/// # Errors
/// Fails on a line that does not name a capability, or on an argument that is
/// not a `--name value` pair.
pub fn parse(args: &[String]) -> Result<Invocation, ParseError> {
    let mut words = args.iter();
    let group = words.next().ok_or(ParseError::NotACapability)?;
    let verb = words.next().ok_or(ParseError::NotACapability)?;
    if group.starts_with('-') || verb.starts_with('-') {
        return Err(ParseError::NotACapability);
    }

    let mut input = std::collections::BTreeMap::new();
    let mut json = false;
    let rest: Vec<&String> = words.collect();
    let mut i = 0;
    while i < rest.len() {
        let word = rest[i];
        let Some(name) = word.strip_prefix("--") else {
            return Err(ParseError::Unexpected {
                word: word.clone(),
            });
        };
        if name == "json" {
            json = true;
            i += 1;
            continue;
        }
        // `--name=value` as well as `--name value`, because both are what
        // people type and refusing one of them is a papercut with no upside.
        if let Some((name, inline)) = name.split_once('=') {
            input.insert(name.replace('-', "_"), value(inline));
            i += 1;
            continue;
        }
        let Some(next) = rest.get(i + 1) else {
            return Err(ParseError::DanglingFlag {
                flag: word.clone(),
            });
        };
        // Repeating a flag builds a list, which is how a capability taking a
        // vector is reachable without a second syntax: `--until idle --until
        // blocked`.
        let key = name.replace('-', "_");
        let parsed = value(next);
        match input.entry(key) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(parsed);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let existing = slot.get_mut();
                if let serde_json::Value::Array(items) = existing {
                    items.push(parsed);
                } else {
                    *existing = serde_json::Value::Array(vec![existing.take(), parsed]);
                }
            }
        }
        i += 2;
    }

    Ok(Invocation {
        capability: format!("{group}.{verb}"),
        input,
        json,
    })
}

/// Run a parsed invocation against a daemon, and report what to exit with.
///
/// Output goes to stdout and everything else to stderr, so a pipe carries data
/// and nothing else.
///
/// # Errors
/// Never: every failure is an exit code and a line on stderr, because a script
/// branches on the code rather than on a message.
pub fn run(args: &[String]) -> Exit {
    let invocation = match parse(args) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("omt: {error}");
            return Exit::Usage;
        }
    };

    let socket = crate::attach::daemon_socket_path();
    let env = std::env::var_os("OMT_SOCK").map(std::path::PathBuf::from);
    let socket = match crate::discovery::discover(
        env.as_deref(),
        &crate::discovery::runtime_dir(),
        &|p| omt_transport::connect(p).is_ok(),
        &mut |stale| {
            let _ = std::fs::remove_file(stale);
        },
    ) {
        crate::discovery::Found::One(path) => path,
        crate::discovery::Found::Several(sockets) => {
            eprintln!("omt: {}", crate::discovery::ambiguous(&sockets));
            // A prompt would have been the interactive answer, and there is no
            // terminal to prompt on in a pipe.
            return Exit::NoTty;
        }
        crate::discovery::Found::None => socket,
    };

    let mut daemon = match crate::attach::Attached::connect(&socket) {
        Ok(daemon) => daemon,
        Err(error) => {
            eprintln!("omt: cannot reach an instance at {}: {error}", socket.display());
            return Exit::Unreachable;
        }
    };

    let input = serde_json::Value::Object(invocation.input.into_iter().collect());
    // Whether it mutates decides whether an intent id is minted, and only the
    // instance knows: asking it is one round trip against a guess that would be
    // wrong for every capability added after this line was written.
    let command = !daemon.is_query(&invocation.capability).unwrap_or(false);
    match daemon.call(&invocation.capability, input, command) {
        Ok(output) => {
            println!("{output}");
            Exit::Ok
        }
        Err(error) => {
            eprintln!("omt: {error}");
            Exit::Failed
        }
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

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_group_and_a_verb_name_a_capability() {
        let out = parse(&args("session list")).expect("parsed");
        assert_eq!(out.capability, "session.list");
    }

    #[test]
    fn flags_become_input_fields() {
        let out = parse(&args("session capture --session s_1 --lines 40")).expect("parsed");
        assert_eq!(out.input["session"], serde_json::json!("s_1"));
        assert_eq!(
            out.input["lines"],
            serde_json::json!(40),
            "a schema wanting an integer will not take a string"
        );
    }

    #[test]
    fn a_dash_in_a_flag_is_the_underscore_in_the_field() {
        // `--include-finished` is what a person types; `include_finished` is
        // what the schema calls it. Making the caller know the difference would
        // be leaking the wire format into the command line.
        let out = parse(&args("task list --include-finished true")).expect("parsed");
        assert_eq!(out.input["include_finished"], serde_json::json!(true));
    }

    #[test]
    fn a_repeated_flag_builds_a_list() {
        // How a capability taking a vector is reachable without a second
        // syntax: `--until idle --until blocked`.
        let out = parse(&args("agent wait --until idle --until blocked")).expect("parsed");
        assert_eq!(out.input["until"], serde_json::json!(["idle", "blocked"]));
    }

    #[test]
    fn equals_and_a_space_mean_the_same_thing() {
        // Both are what people type, and refusing one is a papercut with no
        // upside.
        let spaced = parse(&args("session capture --session s_1")).expect("parsed");
        let joined = parse(&args("session capture --session=s_1")).expect("parsed");
        assert_eq!(spaced.input, joined.input);
    }

    #[test]
    fn json_is_a_flag_rather_than_an_input_field() {
        let out = parse(&args("session list --json")).expect("parsed");
        assert!(out.json);
        assert!(out.input.is_empty(), "it is not sent to the capability");
    }

    #[test]
    fn a_flag_with_nothing_after_it_is_a_usage_error() {
        // Rather than sending a null and having the schema complain about
        // something the caller did not write.
        let error = parse(&args("session capture --session")).expect_err("refused");
        assert_eq!(
            error,
            ParseError::DanglingFlag {
                flag: "--session".to_owned()
            }
        );
    }

    #[test]
    fn a_bare_word_where_a_flag_belongs_is_refused_with_the_shape() {
        let error = parse(&args("session capture s_1")).expect_err("refused");
        assert!(error.to_string().contains("--name value"), "{error}");
    }

    #[test]
    fn a_line_that_names_no_capability_says_what_one_looks_like() {
        // "unrecognised arguments" is a dead end for somebody who has just
        // typed their first command.
        let error = parse(&args("session")).expect_err("refused");
        assert!(error.to_string().contains("omt session list"), "{error}");
    }

    #[test]
    fn a_flag_before_the_verb_is_not_a_capability() {
        assert_eq!(
            parse(&args("--json session list")).expect_err("refused"),
            ParseError::NotACapability
        );
    }

    #[test]
    fn a_value_that_looks_like_json_stays_a_string() {
        // Quoting a path that begins with `{` should not turn it into an
        // object, and a template string is a real thing to pass.
        let out = parse(&args("config set --value {workspace}")).expect("parsed");
        assert_eq!(out.input["value"], serde_json::json!("{workspace}"));
    }

    #[test]
    fn booleans_are_booleans_and_not_the_words() {
        let out = parse(&args("instance shutdown --force true")).expect("parsed");
        assert_eq!(out.input["force"], serde_json::json!(true));
    }
}

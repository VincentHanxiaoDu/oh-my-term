//! The omt binary.
//!
//! It links every crate that declares a capability, which is what makes it the
//! right place to dump the catalog from: the dump is then the list the process
//! actually registers, not a list re-derived by reading source.

use anyhow::Result;
use omt::capabilities;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["debug", "catalog-dump", "--json"] => {
            println!("{}", capabilities::dump()?);
            Ok(())
        }
        ["--version"] => {
            // The commit too. Without it, somebody holding a binary cannot
            // tell whether it contains the fix they were told about, and a
            // session started before an install goes on running the old code
            // with no way to see that it is.
            println!(
                "omt {} ({})",
                env!("CARGO_PKG_VERSION"),
                env!("OMT_BUILD_COMMIT")
            );
            Ok(())
        }
        // The default: run a shell here. Everything else is a subcommand, so
        // `omt` on its own does the thing the name promises.
        [] => omt::run::run_shell(None),
        ["run", program] => omt::run::run_shell(Some((*program).to_owned())),
        ["web"] => omt::web::run(omt_server::DEFAULT_BIND),
        ["web", "--bind", addr] => omt::web::run(addr),
        ["web", "--bind", addr, "--tls-cert", cert, "--tls-key", key] => omt::web::run_with_tls(
            addr,
            Some(omt_server::TlsFiles {
                cert: std::path::PathBuf::from(cert),
                key: std::path::PathBuf::from(key),
            }),
        ),
        ["attach"] => omt::attach::run(None, None),
        ["attach", "--socket", path] => omt::attach::run(Some(std::path::Path::new(path)), None),
        // One process, one instance, both surfaces. Without this the only way
        // to reach a browser was `omt web`, which starts a *second* instance
        // with none of the daemon's sessions in it — so the flow the guide
        // describes ("run omt, then look at your phone") showed an empty
        // roster and looked like omt had lost the work.
        ["serve", "--web", addr] => {
            let path = omt::attach::daemon_socket_path();
            let state = omt::state::State::default();
            let served = state.clone();
            let bind = (*addr).to_owned();
            std::thread::spawn(move || {
                if let Err(e) = omt::web::serve_state(&bind, None, served) {
                    eprintln!("omt: the browser surface stopped: {e}");
                }
            });
            println!("listening on {}", path.display());
            omt::serve::serve(&path, state)
        }
        ["serve"] => {
            // The stable per-user path, so a terminal can find this daemon.
            // The old default had the process id in it, which meant nothing
            // could ever attach to a running one.
            let path = omt::attach::daemon_socket_path();
            println!("listening on {}", path.display());
            omt::serve::serve(&path, omt::state::State::default())
        }
        // Run on the far side of an ssh connection, not by a person.
        ["bridge"] => omt::serve::bridge(None),
        ["bridge", "--socket", path] => omt::serve::bridge(Some(std::path::Path::new(path))),
        ["ssh", destination] => omt::remote::attach(destination),
        ["serve", "--socket", path] => {
            omt::serve::serve(std::path::Path::new(path), omt::state::State::default())
        }
        // Everything else is a capability. The subcommand tree is derived
        // from the catalog rather than written out here (03 §1, artifact 5),
        // so a capability is reachable from a shell the moment it is declared.
        other => {
            let code = omt::cli::run(&other.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
            if code == omt::automation::Exit::Ok {
                Ok(())
            } else {
                // The message is already on stderr; the code is what a script
                // branches on, and anyhow would replace it with 1.
                std::process::exit(i32::from(code.code()))
            }
        }
    }
}

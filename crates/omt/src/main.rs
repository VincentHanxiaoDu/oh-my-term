//! The omt binary.
//!
//! It links every crate that declares a capability, which is what makes it the
//! right place to dump the catalog from: the dump is then the list the process
//! actually registers, not a list re-derived by reading source.

use anyhow::{Result, bail};
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
            println!("omt {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        // The default: run a shell here. Everything else is a subcommand, so
        // `omt` on its own does the thing the name promises.
        [] => omt::run::run_shell(None),
        ["run", program] => omt::run::run_shell(Some((*program).to_owned())),
        ["serve"] => {
            let path = omt::serve::default_socket_path();
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
        other => bail!("unrecognised arguments: {other:?}"),
    }
}

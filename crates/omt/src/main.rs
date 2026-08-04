//! The omt binary.

fn main() -> anyhow::Result<()> {
    println!("omt {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}

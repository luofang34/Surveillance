//! Host command for deterministic replay of recorded reception streams.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::command::ReplayCommand;
use crate::output::OutputFormat;

mod command;
mod output;

#[derive(Debug, Parser)]
#[command(version, about = "Replay recorded receptions through Surveillance")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Replay one newline-delimited reception stream.
    Replay(ReplayArguments),
}

#[derive(Args, Debug)]
struct ReplayArguments {
    /// File that contains newline-delimited ReceptionRecord values.
    input: PathBuf,
    /// File to write. Standard output is the default.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Output representation.
    #[arg(long, value_enum, default_value_t = OutputFormat::TrackRecord)]
    format: OutputFormat,
    /// Fixed producer identity for a reproducible output stream.
    #[arg(long)]
    producer_instance_id: Option<u64>,
}

impl From<ReplayArguments> for ReplayCommand {
    fn from(arguments: ReplayArguments) -> Self {
        Self {
            input: arguments.input,
            output: arguments.output,
            format: arguments.format,
            producer_instance_id: arguments.producer_instance_id,
        }
    }
}

#[allow(
    clippy::disallowed_types,
    reason = "the host CLI can use anyhow at its main boundary"
)]
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|error| anyhow::anyhow!("could not initialize tracing: {error}"))?;

    match Cli::parse().command {
        Command::Replay(arguments) => command::run(arguments.into())?,
    }
    Ok(())
}

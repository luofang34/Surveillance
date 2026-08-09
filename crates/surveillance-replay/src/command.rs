//! File and engine ownership for one replay command.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use surveillance_aero_link::NormalizeError;
use surveillance_aero_link::replay::{ReplayCounts, ReplayError, replay_records};
use surveillance_core::{EngineConfig, ProducerInstanceId, SurveillanceEngine};
use thiserror::Error;
use tracing::{info, warn};

use crate::output::{DeltaWriter, OutputError, OutputFormat};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub(crate) struct ReplayCommand {
    pub(crate) input: PathBuf,
    pub(crate) output: Option<PathBuf>,
    pub(crate) format: OutputFormat,
    pub(crate) producer_instance_id: Option<u64>,
}

pub(crate) fn run(command: ReplayCommand) -> Result<(), ReplayCommandError> {
    command
        .format
        .ensure_available()
        .map_err(ReplayCommandError::OutputFormat)?;
    let producer_instance_id = resolve_producer_instance_id(command.producer_instance_id)?;
    let input = File::open(&command.input).map_err(|source| ReplayCommandError::OpenInput {
        path: command.input.clone(),
        source,
    })?;
    ensure_distinct_output(&command.input, command.output.as_deref())?;
    let destination = destination_name(command.output.as_deref());
    let output = open_output(command.output)?;

    let counts = replay_stream(
        BufReader::new(input),
        BufWriter::new(output),
        producer_instance_id,
        command.format,
    )
    .map_err(|source| ReplayCommandError::Run {
        input: command.input,
        destination,
        source: Box::new(source),
    })?;
    report_counts(counts, producer_instance_id);
    Ok(())
}

fn open_output(path: Option<PathBuf>) -> Result<Box<dyn Write>, ReplayCommandError> {
    match path {
        Some(path) => File::create(&path)
            .map(|file| Box::new(file) as Box<dyn Write>)
            .map_err(|source| ReplayCommandError::CreateOutput { path, source }),
        None => Ok(Box::new(std::io::stdout())),
    }
}

fn ensure_distinct_output(input: &Path, output: Option<&Path>) -> Result<(), ReplayCommandError> {
    let Some(output) = output else {
        return Ok(());
    };
    let same_path = input == output
        || std::fs::canonicalize(input)
            .ok()
            .zip(std::fs::canonicalize(output).ok())
            .is_some_and(|(input, output)| input == output);
    if same_path {
        return Err(ReplayCommandError::InputIsOutput {
            path: input.to_path_buf(),
        });
    }
    Ok(())
}

fn destination_name(path: Option<&Path>) -> String {
    path.map_or_else(
        || String::from("standard output"),
        |path| path.display().to_string(),
    )
}

fn resolve_producer_instance_id(
    fixed: Option<u64>,
) -> Result<ProducerInstanceId, ReplayCommandError> {
    if let Some(id) = fixed {
        return Ok(ProducerInstanceId::new(id));
    }

    let mut bytes = [0_u8; size_of::<u64>()];
    getrandom::fill(&mut bytes)
        .map_err(|source| ReplayCommandError::ProducerIdentity { source })?;
    Ok(ProducerInstanceId::new(u64::from_le_bytes(bytes)))
}

fn replay_stream<R, W>(
    stream: R,
    writer: W,
    producer_instance_id: ProducerInstanceId,
    format: OutputFormat,
) -> Result<ReplayCounts, ReplayRunError>
where
    R: BufRead,
    W: Write,
{
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), producer_instance_id);
    let mut output = DeltaWriter::new(writer, format)?;
    let result = replay_records(
        stream,
        &mut engine,
        |delta| output.write_delta(delta),
        report_refusal,
    );
    output.finish()?;
    result.map_err(ReplayRunError::Replay)
}

fn report_refusal(line: u64, error: &NormalizeError) {
    warn!(line, error = %error, "reception refused");
}

fn report_counts(counts: ReplayCounts, producer_instance_id: ProducerInstanceId) {
    info!(
        producer_instance_id = producer_instance_id.get(),
        receptions_consumed = counts.receptions_consumed,
        observations_produced = counts.observations_produced,
        receptions_skipped_not_traffic = counts.receptions_skipped_not_traffic,
        receptions_refused = counts.receptions_refused,
        "replay complete"
    );
}

#[derive(Debug, Error)]
pub(crate) enum ReplayCommandError {
    #[error("the selected output format is not available")]
    OutputFormat(#[source] OutputError),
    #[error("could not open reception recording {}", path.display())]
    OpenInput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the replay output cannot overwrite its input {}", path.display())]
    InputIsOutput { path: PathBuf },
    #[error("could not create replay output {}", path.display())]
    CreateOutput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not assign a producer instance identity")]
    ProducerIdentity {
        #[source]
        source: getrandom::Error,
    },
    #[error("could not replay {} to {destination}", input.display())]
    Run {
        input: PathBuf,
        destination: String,
        #[source]
        source: Box<ReplayRunError>,
    },
}

#[derive(Debug, Error)]
pub(crate) enum ReplayRunError {
    #[error("the reception stream failed")]
    Replay(#[source] ReplayError),
    #[error("the output stream failed")]
    Output(#[from] OutputError),
}

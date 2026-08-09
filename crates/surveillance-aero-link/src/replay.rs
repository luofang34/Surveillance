//! Driving the engine from a recorded reception stream.
//!
//! A recording holds one `ReceptionRecord` on each line. That record is a
//! versioned envelope, so a reader examines the declared schema version
//! before it decodes the event inside. A recording that declares a version
//! this build does not support is refused, and its payload is never
//! interpreted as a shape it may no longer have.
//!
//! The reader keeps the traffic-only contract of this crate. On 978 MHz most
//! receptions are ground uplinks that carry weather and notices. Such a
//! reception has no traffic projection, so the reader counts it as skipped
//! and reports no error. Weather has no path into the engine.
//!
//! A stream fault and a reception fault are different. A line that does not
//! decode, or a record from an unsupported schema, makes the remainder of
//! the recording untrustworthy, so the reader stops. A single traffic
//! reception that names no target the adapter can normalize leaves the
//! stream intact, so the reader counts it and continues.

#[cfg(feature = "std")]
use std::io::BufRead;

use aero_link::ReceptionEvent;
#[cfg(feature = "std")]
use aero_link::{
    ReceptionRecord, ReceptionRecordDecodeError, ReceptionRecordHeader,
    decode_reception_record_with,
};
use surveillance_core::{IngestError, SurveillanceEngine, TrackDelta};
#[cfg(feature = "std")]
use thiserror::Error;

use crate::{NormalizeError, normalize};

/// What one reception did to the engine.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceptionOutcome {
    /// The reception carried traffic, and one observation reached the engine.
    Ingested,
    /// The reception carried no traffic, so engine state did not change.
    ///
    /// This is the usual result for a ground uplink, and it is not a failure.
    NotTraffic,
    /// The reception carried traffic that names no target this adapter can
    /// normalize, so engine state did not change.
    Refused(NormalizeError),
}

/// Tally of what one reception stream did to the engine.
///
/// [`Self::receptions_consumed`] is always the sum of the three other
/// counters, because each reception has exactly one outcome.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReplayCounts {
    /// Receptions read from the stream.
    pub receptions_consumed: u64,
    /// Observations that reached the engine.
    pub observations_produced: u64,
    /// Receptions that carry no traffic, thus weather and other uplink
    /// content.
    pub receptions_skipped_not_traffic: u64,
    /// Traffic receptions the adapter refused to normalize.
    pub receptions_refused: u64,
}

impl ReplayCounts {
    /// Count one reception and the outcome it produced.
    ///
    /// A caller that reads a live stream rather than a recording uses this
    /// with [`ingest_event`] to tally the same quantities the reader reports.
    pub fn record(&mut self, outcome: &ReceptionOutcome) {
        self.receptions_consumed = self.receptions_consumed.wrapping_add(1);
        let counter = match outcome {
            ReceptionOutcome::Ingested => &mut self.observations_produced,
            ReceptionOutcome::NotTraffic => &mut self.receptions_skipped_not_traffic,
            ReceptionOutcome::Refused(_) => &mut self.receptions_refused,
        };
        *counter = counter.wrapping_add(1);
    }
}

/// Feed one decoded reception to the engine and report what it did.
///
/// A reception that carries no traffic gives [`ReceptionOutcome::NotTraffic`]
/// and not an error, because the caller must not have to tell weather apart
/// from a genuine failure. Only the engine itself produces an error here.
pub fn ingest_event(
    event: &ReceptionEvent,
    engine: &mut SurveillanceEngine,
    on_delta: impl FnMut(&TrackDelta),
) -> Result<ReceptionOutcome, IngestError> {
    let Some(projection) = event.traffic() else {
        return Ok(ReceptionOutcome::NotTraffic);
    };
    let observation = match normalize(&projection) {
        Ok(observation) => observation,
        Err(error) => return Ok(ReceptionOutcome::Refused(error)),
    };
    engine.ingest(observation, on_delta)?;
    Ok(ReceptionOutcome::Ingested)
}

/// Read a newline-delimited stream of reception records and drive `engine`
/// with the traffic in it.
///
/// Each non-empty line must hold one JSON `ReceptionRecord`. The reader
/// stops at the first line it cannot decode and at the first record from an
/// unsupported schema, because the remainder of such a recording cannot be
/// trusted. It also stops if the engine refuses an observation, because a
/// full track table drops aircraft the caller asked to see.
///
/// # Errors
///
/// Returns [`ReplayError`] with the one-based line that caused the failure.
#[cfg(feature = "std")]
pub fn replay_records<R, F>(
    stream: R,
    engine: &mut SurveillanceEngine,
    mut on_delta: F,
) -> Result<ReplayCounts, ReplayError>
where
    R: BufRead,
    F: FnMut(&TrackDelta),
{
    let mut counts = ReplayCounts::default();
    let mut line_number = 0_u64;
    for line in stream.lines() {
        line_number = line_number.wrapping_add(1);
        let line = line.map_err(|source| ReplayError::Read {
            line: line_number,
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let event = decode_event(line.as_bytes()).map_err(|source| ReplayError::Decode {
            line: line_number,
            source,
        })?;
        let outcome =
            ingest_event(&event, engine, &mut on_delta).map_err(|source| ReplayError::Ingest {
                line: line_number,
                source,
            })?;
        counts.record(&outcome);
    }
    Ok(counts)
}

/// Decode one record in two phases, so an unsupported schema version is
/// refused before Serde reads a payload whose shape may have changed.
#[cfg(feature = "std")]
fn decode_event(
    line: &[u8],
) -> Result<ReceptionEvent, ReceptionRecordDecodeError<serde_json::Error>> {
    let record = decode_reception_record_with(
        line,
        serde_json::from_slice::<ReceptionRecordHeader>,
        serde_json::from_slice::<ReceptionRecord>,
    )?;
    Ok(record.into_event()?)
}

/// Failure reading a recorded reception stream.
#[cfg(feature = "std")]
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ReplayError {
    /// The stream could not supply a line.
    #[error("could not read line {line} of the recorded stream")]
    Read {
        /// One-based position of the line in the stream.
        line: u64,
        /// Failure reported by the stream.
        #[source]
        source: std::io::Error,
    },
    /// A line does not hold a reception record this build can interpret.
    #[error("line {line} of the recorded stream is not a supported reception record")]
    Decode {
        /// One-based position of the line in the stream.
        line: u64,
        /// Failure reported by the record decoder.
        #[source]
        source: ReceptionRecordDecodeError<serde_json::Error>,
    },
    /// The engine refused an observation the adapter normalized.
    #[error("the engine refused the observation from line {line}")]
    Ingest {
        /// One-based position of the line in the stream.
        line: u64,
        /// Failure reported by the engine.
        #[source]
        source: IngestError,
    },
}

//! Driving the engine from a recorded reception stream.
//!
//! A recording holds one `ReceptionRecord` on each line. That record is a
//! versioned envelope. The reader examines the declared schema version, and
//! then it decodes the event. This order is the reason the envelope exists.
//! The reader refuses a record from a version that this build does not
//! support, and it does not decode the payload of that record. The payload
//! can have a different structure in a different version.
//!
//! The reader keeps the traffic-only contract of this crate. On 978 MHz most
//! receptions are ground uplinks that carry weather and notices. Such a
//! reception has no traffic projection. The reader counts it as skipped and
//! reports no error, because a ground uplink is not a failure.
//!
//! The reader also drives time. A recorded arrival time is monotonic runtime
//! time, and the engine expires a track only when the caller moves time
//! forward. A reader that ingests but never advances time gives a delta
//! stream that no live run can produce: no track coasts, no track expires,
//! and the track table grows for the full length of the recording.
//!
//! The reader divides a stream fault from a reception fault:
//!
//! - A line that does not decode, and a record from a schema or a band that
//!   this build does not support, make the remainder of the recording
//!   unreliable. The reader stops and names the line.
//! - One traffic reception that names no target leaves the stream intact.
//!   The reader counts it, reports the reason to the caller, and continues.

#[cfg(feature = "std")]
use std::io::{BufRead, Read};

use aero_link::ReceptionEvent;
#[cfg(feature = "std")]
use aero_link::{
    ReceptionRecord, ReceptionRecordDecodeError, ReceptionRecordHeader,
    decode_reception_record_with,
};
#[cfg(feature = "std")]
use surveillance_core::SnapshotPublicationError;
use surveillance_core::{IngestError, SurveillanceEngine, TrackDelta};
#[cfg(feature = "std")]
use thiserror::Error;

use crate::{NormalizeError, normalize};

#[cfg(test)]
mod tests;

/// Largest line the reader accepts, in bytes.
///
/// A recorded line holds one reception, which is far smaller than this. The
/// limit stops a truncated recording, or two recordings joined without a
/// newline between them, from becoming one unbounded read.
pub const MAX_RECORD_LINE_BYTES: usize = 1 << 20;

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
    /// Receptions that carry no traffic, which is weather and other uplink
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
/// and not an error. A ground uplink is the normal input on 978 MHz, and the
/// caller must not have to separate it from a genuine failure. Only the
/// engine itself produces an error here.
///
/// This function applies no stream policy. It advances no time, and it treats
/// every refusal alike. A caller that reads a live stream decides both, in
/// the same way `replay_records` decides them for a recording. That reader
/// needs an I/O trait, so the `std` feature gates it and this function stays
/// available to a portable build.
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

/// Report whether a refusal ends the stream or only one reception.
///
/// A band this build cannot normalize is the same class of problem as a
/// schema version it cannot interpret: the recording holds input that this
/// build is older than. To count such a reception would report a successful
/// replay that quietly dropped one whole input source. Every other refusal
/// names one target the adapter cannot identify, which the next reception
/// does not inherit.
///
/// A caller that reads a live stream through [`ingest_event`] applies the
/// same rule with this function, so a live run and a replay agree.
///
/// The match is exhaustive on purpose. A wildcard would put each refusal
/// added upstream on the per-reception side without a decision, which is the
/// error this rule exists to prevent.
#[must_use]
pub fn refusal_ends_the_stream(error: &NormalizeError) -> bool {
    match error {
        NormalizeError::UnsupportedPayload => true,
        NormalizeError::NoTargetIdentity { .. }
        | NormalizeError::UnconfirmedAddress { .. }
        | NormalizeError::ServiceManagement
        | NormalizeError::ReservedAddressQualifier { .. }
        | NormalizeError::Squawk(_) => false,
    }
}

/// Read a newline-delimited stream of reception records and drive `engine`
/// with the traffic in it.
///
/// Each non-empty line must hold one JSON `ReceptionRecord`. `on_delta`
/// receives every track delta. `on_refusal` receives the line and the reason
/// for each traffic reception the reader refused and then continued past.
/// Refusals are normal on 1090 MHz, where frames that name no target still
/// have a traffic projection, so the reason is what separates ordinary
/// refusals from a change upstream that refuses everything. A refusal that
/// ends the stream does not reach `on_refusal`, because the error carries it
/// and the tally does not count it.
///
/// Time moves forward with the recording. After each reception the reader
/// advances the engine to the largest arrival time it has read, so a
/// recording produces the coasting and removal deltas that its length calls
/// for. The reader never reads a clock.
///
/// One recording is one runtime clock. Receptions can arrive a little out of
/// order, and the largest arrival time absorbs that. A reception that is
/// older than the removal window is different: the engine would expire the
/// track it makes before the next reception arrives. Such a step backwards
/// is evidence of two recordings joined together, or of a damaged arrival
/// time, so the reader stops.
///
/// # Errors
///
/// Returns [`ReplayError`] with the one-based line that failed and the tally
/// of the lines before it.
#[cfg(feature = "std")]
pub fn replay_records<R, F, G>(
    mut stream: R,
    engine: &mut SurveillanceEngine,
    mut on_delta: F,
    mut on_refusal: G,
) -> Result<ReplayCounts, ReplayError>
where
    R: BufRead,
    F: FnMut(&TrackDelta),
    G: FnMut(u64, &NormalizeError),
{
    let mut counts = ReplayCounts::default();
    let mut clock_micros = 0_u64;
    let mut line = Vec::new();
    let mut line_number = 0_u64;
    let backward_limit = engine.config().freshness.remove_after_micros;

    loop {
        line.clear();
        line_number = line_number.wrapping_add(1);
        let fault = |kind| ReplayError {
            line: line_number,
            counts,
            kind,
        };

        if read_bounded_line(&mut stream, &mut line).map_err(fault)? == 0 {
            return Ok(counts);
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }

        let event = decode_event(&line).map_err(fault)?;
        let arrival_micros = event.received_at_micros;
        if arrival_micros.saturating_add(backward_limit) < clock_micros {
            return Err(fault(ReplayFault::ArrivalWentBackwards {
                arrival_micros,
                clock_micros,
            }));
        }
        clock_micros = clock_micros.max(arrival_micros);
        let outcome = ingest_event(&event, engine, &mut on_delta)
            .map_err(|source| fault(ReplayFault::Ingest { source }))?;
        if let ReceptionOutcome::Refused(error) = &outcome {
            if refusal_ends_the_stream(error) {
                return Err(fault(ReplayFault::UnsupportedBand { source: *error }));
            }
            on_refusal(line_number, error);
        }
        engine
            .advance_time(clock_micros, &mut on_delta)
            .map_err(|source| fault(ReplayFault::AdvanceTime { source }))?;
        counts.record(&outcome);
    }
}

/// Read one line and remove its terminator, refusing a line whose content is
/// longer than [`MAX_RECORD_LINE_BYTES`].
///
/// Returns the number of bytes read, which is zero at the end of the stream.
///
/// The read stops two bytes beyond the limit, which is one byte of content
/// more than the limit allows plus the longest terminator. The length is
/// measured after the terminator goes, so the limit means the same for a
/// recording that ends a line with `\n` and for one that ends it with `\r\n`.
#[cfg(feature = "std")]
fn read_bounded_line(stream: &mut impl BufRead, line: &mut Vec<u8>) -> Result<usize, ReplayFault> {
    let ceiling = (MAX_RECORD_LINE_BYTES as u64).saturating_add(3);
    let read = stream
        .take(ceiling)
        .read_until(b'\n', line)
        .map_err(|source| ReplayFault::Read { source })?;
    strip_terminator(line);
    if line.len() > MAX_RECORD_LINE_BYTES {
        return Err(ReplayFault::LineTooLong {
            limit: MAX_RECORD_LINE_BYTES,
        });
    }
    Ok(read)
}

/// Remove the line terminator, which is `\n` or `\r\n`.
#[cfg(feature = "std")]
fn strip_terminator(line: &mut Vec<u8>) {
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
}

/// Decode one record in two phases, so an unsupported schema version is
/// refused before Serde reads a payload whose structure may have changed.
///
/// Bytes that are not valid UTF-8 fail here rather than in the read, because
/// a damaged record is a fault in the recording and not in the transport
/// that supplied it.
#[cfg(feature = "std")]
fn decode_event(line: &[u8]) -> Result<ReceptionEvent, ReplayFault> {
    let record = decode_reception_record_with(
        line,
        serde_json::from_slice::<ReceptionRecordHeader>,
        serde_json::from_slice::<ReceptionRecord>,
    )
    .map_err(|source| ReplayFault::Decode { source })?;
    record.into_event().map_err(|error| ReplayFault::Decode {
        source: ReceptionRecordDecodeError::Schema(error),
    })
}

/// Failure reading a recorded reception stream.
///
/// The tally counts the lines before the failure. The engine keeps every
/// change those lines already applied, so a caller can report what it
/// ingested before it stopped.
#[cfg(feature = "std")]
#[derive(Debug, Error)]
#[error("the recorded reception stream failed on line {line}")]
pub struct ReplayError {
    /// One-based position of the line that failed.
    pub line: u64,
    /// Tally of the lines read before this one.
    pub counts: ReplayCounts,
    /// What made the stream stop.
    #[source]
    pub kind: ReplayFault,
}

/// What made a recorded reception stream stop.
#[cfg(feature = "std")]
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ReplayFault {
    /// The stream could not supply a line.
    #[error("the stream could not supply the line")]
    Read {
        /// Failure reported by the stream.
        #[source]
        source: std::io::Error,
    },
    /// The line is longer than one reception record can be.
    #[error("the line is longer than the {limit} byte limit for one record")]
    LineTooLong {
        /// Value of [`MAX_RECORD_LINE_BYTES`].
        limit: usize,
    },
    /// The line does not hold a reception record this build can interpret.
    #[error("the line is not a supported reception record")]
    Decode {
        /// Failure reported by the record decoder.
        #[source]
        source: ReceptionRecordDecodeError<serde_json::Error>,
    },
    /// The arrival time goes back further than one runtime clock can go.
    ///
    /// The engine would expire the track this reception makes before the next
    /// reception arrives, so the recording is not one continuous stream.
    #[error(
        "arrival time {arrival_micros} is older than the clock {clock_micros} by more than one removal window"
    )]
    ArrivalWentBackwards {
        /// Arrival time declared by the reception on this line.
        arrival_micros: u64,
        /// Largest arrival time read before this line.
        clock_micros: u64,
    },
    /// The record names a band this build does not normalize.
    #[error("the record names a band this build does not normalize")]
    UnsupportedBand {
        /// Refusal reported by the adapter.
        #[source]
        source: NormalizeError,
    },
    /// The engine refused an observation the adapter normalized.
    #[error("the engine refused the observation")]
    Ingest {
        /// Failure reported by the engine.
        #[source]
        source: IngestError,
    },
    /// The engine could not publish a lifecycle transition.
    #[error("the engine could not advance track time")]
    AdvanceTime {
        /// Publication failure reported by the engine.
        #[source]
        source: SnapshotPublicationError,
    },
}

//! Which failures stop a recorded stream, and what each one reports.

#![cfg(feature = "std")]
#![allow(clippy::expect_used, clippy::panic)]

#[path = "support/reception_records.rs"]
mod records;

use std::io::{self, BufRead, Read};

use aero_link::{ReceptionRecordDecodeError, ReceptionSchemaError};
use records::{
    EVEN_POSITION, ODD_POSITION, OTHER_AIRCRAFT, SUPPORTED_SCHEMA, position_event, recorded,
    replay, replay_with,
};
use surveillance_aero_link::replay::{MAX_RECORD_LINE_BYTES, ReplayFault, replay_records};
use surveillance_core::{EngineConfig, FreshnessPolicy, ProducerInstanceId, SurveillanceEngine};

/// The two-phase decode exists so an unsupported schema is refused before its
/// payload is read. The payload here cannot deserialize into an event at all,
/// so a `Decode` failure carrying `Schema` proves the payload decoder never
/// ran.
#[test]
fn an_unsupported_schema_version_is_refused_before_the_payload_is_decoded() {
    let stream = "{\"schema_version\":5,\"event\":\"this is not an event\"}\n";

    let replayed = replay(stream);
    let error = replayed.outcome.expect_err("version 5 is unsupported");

    assert_eq!(error.line, 1);
    match error.kind {
        ReplayFault::Decode {
            source:
                ReceptionRecordDecodeError::Schema(ReceptionSchemaError::UnsupportedVersion {
                    supported,
                    actual,
                }),
        } => {
            assert_eq!(supported, SUPPORTED_SCHEMA);
            assert_eq!(actual, 5);
        }
        other => panic!("expected a schema refusal before payload decoding, got {other:?}"),
    }
    assert_eq!(replayed.engine.track_count(), 0);
}

/// A recorder that writes a bare event rather than a record produces a line
/// with no declared version. It is refused as version zero rather than read
/// as though it were the current schema.
#[test]
fn a_bare_reception_event_is_refused_as_an_unversioned_record() {
    let event = position_event(1_000, ODD_POSITION);
    let mut stream = serde_json::to_string(&event).expect("an event serializes");
    stream.push('\n');
    assert!(
        !stream.contains("schema_version"),
        "a bare event declares no version"
    );

    let replayed = replay(&stream);
    let error = replayed
        .outcome
        .expect_err("an unversioned line is refused");

    match error.kind {
        ReplayFault::Decode {
            source:
                ReceptionRecordDecodeError::Schema(ReceptionSchemaError::UnsupportedVersion {
                    actual,
                    ..
                }),
        } => assert_eq!(actual, 0, "an absent version is not the current version"),
        other => panic!("expected an unversioned refusal, got {other:?}"),
    }
}

/// A line the decoder cannot read makes the rest of the recording
/// untrustworthy, so the reader stops there, names the line, and hands back
/// the tally of what it had already ingested.
#[test]
fn a_malformed_line_stops_the_stream_and_reports_the_tally_so_far() {
    let mut stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        position_event(2_000, EVEN_POSITION),
    ]);
    stream.push_str("{ this is not JSON\n");
    stream.push_str(&recorded(&[position_event(3_000, OTHER_AIRCRAFT)]));

    let replayed = replay(&stream);
    let error = replayed.outcome.expect_err("a malformed line stops");

    assert_eq!(error.line, 3);
    assert!(matches!(
        error.kind,
        ReplayFault::Decode {
            source: ReceptionRecordDecodeError::Header { .. }
        }
    ));
    assert_eq!(
        error.counts.receptions_consumed, 2,
        "the tally of the lines before the failure survives the error"
    );
    assert_eq!(error.counts.observations_produced, 2);
    assert_eq!(
        replayed.engine.track_count(),
        1,
        "the reader stopped at the bad line, after the good ones"
    );
}

/// A damaged byte inside a record is a fault in the recording. To report it
/// as a read failure would send an operator to examine the file handle.
///
/// The two streams here differ by one byte, and the byte sits inside a JSON
/// string value. The record is thus well formed in every other way, so the
/// damaged byte is the only thing that can account for the difference.
#[test]
fn an_invalid_utf8_byte_inside_a_record_is_a_record_fault() {
    let clean = recorded(&[position_event(1_000, ODD_POSITION)]).into_bytes();
    let transport = clean
        .windows(3)
        .position(|window| window == b"usb")
        .expect("the record names its transport as a JSON string");

    let mut damaged = clean.clone();
    damaged[transport + 1] = 0xFF;

    replay_with(EngineConfig::default(), &clean)
        .outcome
        .expect("the undamaged record replays");

    let error = replay_with(EngineConfig::default(), &damaged)
        .outcome
        .expect_err("the damaged record is refused");
    assert_eq!(error.line, 1);
    assert!(
        matches!(error.kind, ReplayFault::Decode { .. }),
        "a damaged byte is a record fault, not a transport fault, got {:?}",
        error.kind
    );
}

/// A recording joined to another without a newline between them makes one
/// line from two files. To read it whole would pull an unbounded quantity
/// into memory.
#[test]
fn a_line_longer_than_one_record_is_refused_rather_than_read_whole() {
    let mut stream = vec![b'x'; MAX_RECORD_LINE_BYTES + 1];
    stream.push(b'\n');

    let replayed = replay_with(EngineConfig::default(), &stream);
    let error = replayed.outcome.expect_err("an unbounded line is refused");

    match error.kind {
        ReplayFault::LineTooLong { limit } => assert_eq!(limit, MAX_RECORD_LINE_BYTES),
        other => panic!("expected a length refusal, got {other:?}"),
    }
}

/// The limit counts the content of a line and not its terminator, so it means
/// the same for a recording that ends a line with `\n` and for one that ends
/// it with `\r\n`.
#[test]
fn the_length_limit_counts_content_and_not_the_line_terminator() {
    for terminator in ["\n", "\r\n"] {
        let mut at_limit = vec![b'x'; MAX_RECORD_LINE_BYTES];
        at_limit.extend_from_slice(terminator.as_bytes());
        let error = replay_with(EngineConfig::default(), &at_limit)
            .outcome
            .expect_err("a line of x characters is not a record");
        assert!(
            matches!(error.kind, ReplayFault::Decode { .. }),
            "content of exactly the limit must reach the decoder with {terminator:?}, got {:?}",
            error.kind
        );

        let mut past_limit = vec![b'x'; MAX_RECORD_LINE_BYTES + 1];
        past_limit.extend_from_slice(terminator.as_bytes());
        let error = replay_with(EngineConfig::default(), &past_limit)
            .outcome
            .expect_err("one byte more is too long");
        assert!(
            matches!(error.kind, ReplayFault::LineTooLong { .. }),
            "one byte past the limit must be refused with {terminator:?}, got {:?}",
            error.kind
        );
    }
}

/// Two recordings joined with a newline between them are one file whose clock
/// restarts. The engine would expire each track the second recording makes
/// before the next reception arrived, so the reader refuses the step instead
/// of absorbing it.
#[test]
fn a_step_backwards_past_the_removal_window_stops_the_stream() {
    let freshness = FreshnessPolicy::default();
    let stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        position_event(freshness.remove_after_micros + 2_000, EVEN_POSITION),
        // The second recording begins again near zero.
        position_event(1_000, OTHER_AIRCRAFT),
    ]);

    let replayed = replay(&stream);
    let error = replayed.outcome.expect_err("the clock restarted");

    assert_eq!(error.line, 3);
    match error.kind {
        ReplayFault::ArrivalWentBackwards {
            arrival_micros,
            clock_micros,
        } => {
            assert_eq!(arrival_micros, 1_000);
            assert_eq!(clock_micros, freshness.remove_after_micros + 2_000);
        }
        other => panic!("expected a backward clock refusal, got {other:?}"),
    }
    assert_eq!(error.counts.receptions_consumed, 2);
}

/// Receptions can arrive a little out of order. The largest arrival time
/// absorbs that, and the engine never sees the clock go backwards.
#[test]
fn a_small_step_backwards_in_arrival_time_is_absorbed() {
    let stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        position_event(5_000, OTHER_AIRCRAFT),
        position_event(2_000, EVEN_POSITION),
    ]);

    let replayed = replay(&stream);
    let counts = replayed
        .outcome
        .expect("a reception that is slightly late is normal");

    assert_eq!(counts.receptions_consumed, 3);
    assert_eq!(counts.observations_produced, 3);
    assert_eq!(
        replayed.engine.track_count(),
        2,
        "no track expired because of the late reception"
    );
}

/// The engine refuses a new target when its track bound is full. That is a
/// stream fault: to continue would drop aircraft the caller asked to see.
#[test]
fn a_full_track_table_stops_the_stream() {
    let mut config = EngineConfig::default();
    config.max_tracks = Some(1);
    let stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        position_event(2_000, OTHER_AIRCRAFT),
    ]);

    let replayed = replay_with(config, stream.as_bytes());
    let error = replayed.outcome.expect_err("the second target has no room");

    assert_eq!(error.line, 2);
    assert!(
        matches!(error.kind, ReplayFault::Ingest { .. }),
        "expected an engine refusal, got {:?}",
        error.kind
    );
    assert_eq!(error.counts.receptions_consumed, 1);
}

/// A stream that cannot supply a line is a transport fault, and it names the
/// line it failed on.
#[test]
fn a_stream_that_fails_to_read_reports_a_transport_fault() {
    struct BrokenStream;

    impl Read for BrokenStream {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("the receiver went away"))
        }
    }

    impl BufRead for BrokenStream {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::other("the receiver went away"))
        }

        fn consume(&mut self, _: usize) {}
    }

    let mut engine = SurveillanceEngine::new(EngineConfig::default(), ProducerInstanceId::new(1));
    let error = replay_records(BrokenStream, &mut engine, |_| {}, |_, _| {})
        .expect_err("a broken stream is an error");

    assert_eq!(error.line, 1);
    assert!(
        matches!(error.kind, ReplayFault::Read { .. }),
        "expected a transport fault, got {:?}",
        error.kind
    );
}

/// A recording may carry blank lines from concatenation or an editor. They
/// are not receptions, so they must not inflate the tally, and they must not
/// move the line number the reader reports.
#[test]
fn blank_lines_are_skipped_but_still_advance_the_line_number() {
    let mut stream = recorded(&[position_event(1_000, ODD_POSITION)]);
    stream.push_str("\n   \n\t\n");
    stream.push_str(&recorded(&[position_event(2_000, EVEN_POSITION)]));

    let counts = replay(&stream)
        .outcome
        .expect("blank lines are not a failure");
    assert_eq!(counts.receptions_consumed, 2);
    assert_eq!(counts.observations_produced, 2);

    let error = replay("\n   \n\t\n{ not json\n")
        .outcome
        .expect_err("the bad line still fails");
    assert_eq!(error.line, 4, "blank lines still count toward the position");
}

/// A recorder stopped in the middle of a write, or a file joined by hand, can
/// leave the last line without its newline. That line is still a reception.
#[test]
fn a_final_line_without_a_newline_is_still_read() {
    let stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        position_event(2_000, EVEN_POSITION),
    ]);
    let truncated = stream.trim_end_matches('\n');

    let replayed = replay(truncated);
    let counts = replayed.outcome.expect("the last line is complete");

    assert_eq!(counts.receptions_consumed, 2);
    assert_eq!(counts.observations_produced, 2);
    assert_eq!(replayed.engine.track_count(), 1);
}

/// A recording written on a platform that ends a line with CRLF decodes the
/// same as one that ends it with LF.
#[test]
fn a_recording_with_crlf_line_endings_decodes() {
    let stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        position_event(2_000, EVEN_POSITION),
    ])
    .replace('\n', "\r\n");

    let counts = replay(&stream).outcome.expect("CRLF is a line ending");

    assert_eq!(counts.receptions_consumed, 2);
    assert_eq!(counts.observations_produced, 2);
}

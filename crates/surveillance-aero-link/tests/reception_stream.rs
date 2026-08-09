//! Stream contract: a recording drives the engine, and only traffic does.

#![cfg(feature = "std")]
#![allow(clippy::expect_used, clippy::panic)]

use aero_link::{
    AddressQualifier, Adsb1090Message, CorrectedUatFrame, ReceiverSource, ReceptionEvent,
    ReceptionRecord, ReceptionRecordDecodeError, ReceptionSchemaError, SourceTransport, UatDecoder,
    UatDownlink, UatFrameClass, UatMessage, UatReception,
};
use surveillance_aero_link::replay::{ReplayCounts, ReplayError, replay_records};
use surveillance_core::{AddressNamespace, EngineConfig, SurveillanceEngine, TrackDelta, TrackKey};

/// Even and odd halves of a published airborne position pair for one target.
const EVEN_POSITION: &str = "8D40621D58C382D690C8AC2863A7";
const ODD_POSITION: &str = "8D40621D58C386435CC412692AD6";

/// Schema version this build supports, as `aero-link` defines it.
const SUPPORTED_SCHEMA: u16 = 6;

fn decode(hex: &str) -> Adsb1090Message {
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .map(|index| {
            u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("valid hex pair")
        })
        .collect();
    Adsb1090Message::decode(&bytes, None).expect("published frame decodes")
}

fn receiver() -> ReceiverSource {
    ReceiverSource::new(1, SourceTransport::Usb)
}

fn position_event(at: u64, hex: &str) -> ReceptionEvent {
    ReceptionEvent::adsb1090(at, decode(hex)).with_source(receiver())
}

/// A 978 MHz ground uplink: the band's weather and notice carrier, and the
/// reception the engine must never see.
fn ground_uplink(at: u64) -> ReceptionEvent {
    let bytes = vec![0_u8; UatFrameClass::GroundUplink.corrected_bytes()].into_boxed_slice();
    let frame = CorrectedUatFrame::try_from_boxed_bytes(UatFrameClass::GroundUplink, bytes)
        .expect("a sized uplink frame is valid");
    let reception = UatDecoder::default()
        .decode_corrected(frame, -20, 0, 0)
        .expect("a ground uplink decodes");
    assert!(
        matches!(reception.message, UatMessage::Uplink(_)),
        "a 432-byte frame is a ground uplink"
    );
    ReceptionEvent::uat978(at, reception).with_source(receiver())
}

/// A UAT downlink whose address qualifier is reserved. It is traffic, so it
/// reaches the adapter, and the adapter refuses to guess a namespace for it.
fn unnormalizable_downlink(at: u64) -> ReceptionEvent {
    let bytes = vec![0_u8; UatFrameClass::BasicDownlink.corrected_bytes()].into_boxed_slice();
    let reception = UatReception {
        rssi_raw: -20,
        receiver_timestamp: 0,
        corrected_symbols: 0,
        encoded_frame: None,
        corrected_frame: CorrectedUatFrame::try_from_boxed_bytes(
            UatFrameClass::BasicDownlink,
            bytes,
        )
        .expect("a sized downlink frame is valid"),
        message: UatMessage::Downlink(UatDownlink {
            message_type: 1,
            address_qualifier: AddressQualifier::Reserved6,
            address: 0x00AB_CDEF,
            state_vector: None,
            mode_status: None,
            secondary_altitude: None,
        }),
    };
    ReceptionEvent::uat978(at, reception).with_source(receiver())
}

/// Wrap each event in the versioned envelope a recorder writes, one to a line.
fn recorded(events: &[ReceptionEvent]) -> String {
    let mut stream = String::new();
    for event in events {
        let record = ReceptionRecord::new(event.clone());
        stream.push_str(&serde_json::to_string(&record).expect("a record serializes"));
        stream.push('\n');
    }
    stream
}

fn replay(
    stream: &str,
) -> (
    SurveillanceEngine,
    Vec<TrackDelta>,
    Result<ReplayCounts, ReplayError>,
) {
    let mut engine = SurveillanceEngine::new(EngineConfig::default());
    let mut deltas = Vec::new();
    let outcome = replay_records(stream.as_bytes(), &mut engine, |delta| {
        deltas.push(delta.clone())
    });
    (engine, deltas, outcome)
}

fn assert_counts_add_up(counts: ReplayCounts) {
    assert_eq!(
        counts.receptions_consumed,
        counts.observations_produced
            + counts.receptions_skipped_not_traffic
            + counts.receptions_refused,
        "every reception has exactly one outcome: {counts:?}"
    );
}

/// The requirement this repository had only by construction: a stream that
/// mixes ground uplinks with traffic changes engine state for the traffic
/// alone, and reports no error for the rest.
///
/// The skip path is hot, not an edge case. Close to half of all 978 MHz
/// receptions in real air are ground uplinks.
#[test]
fn a_mixed_stream_admits_traffic_and_skips_ground_uplinks_without_error() {
    let stream = recorded(&[
        ground_uplink(500),
        position_event(1_000, ODD_POSITION),
        ground_uplink(1_500),
        position_event(2_000, EVEN_POSITION),
        ground_uplink(2_500),
    ]);

    let (engine, deltas, outcome) = replay(&stream);
    let counts = outcome.expect("a ground uplink is not an error");

    assert_eq!(counts.receptions_consumed, 5);
    assert_eq!(counts.observations_produced, 2, "only traffic is observed");
    assert_eq!(
        counts.receptions_skipped_not_traffic, 3,
        "every ground uplink is skipped"
    );
    assert_eq!(counts.receptions_refused, 0);
    assert_counts_add_up(counts);

    assert_eq!(
        engine.track_count(),
        1,
        "the uplinks left engine state untouched"
    );
    assert!(matches!(deltas.first(), Some(TrackDelta::Created(_))));

    let key = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);
    let snapshot = engine.snapshot(key).expect("the target became a track");
    let position = snapshot.position.as_ref().expect("the CPR pair decoded");
    assert!(
        (position.value.latitude_deg - 52.257_202).abs() < 1e-4
            && (position.value.longitude_deg - 3.919_372).abs() < 1e-4,
        "decoded {:?}",
        position.value
    );
}

/// The same contract with nothing else in the stream, so no traffic can mask
/// a failure on the skip path.
#[test]
fn a_stream_of_ground_uplinks_alone_produces_no_observation_and_no_error() {
    let stream = recorded(&[
        ground_uplink(500),
        ground_uplink(1_000),
        ground_uplink(1_500),
    ]);

    let (engine, deltas, outcome) = replay(&stream);
    let counts = outcome.expect("weather is skipped, not refused");

    assert_eq!(counts.receptions_consumed, 3);
    assert_eq!(counts.observations_produced, 0);
    assert_eq!(counts.receptions_skipped_not_traffic, 3);
    assert_eq!(counts.receptions_refused, 0);
    assert_counts_add_up(counts);
    assert_eq!(engine.track_count(), 0);
    assert!(deltas.is_empty(), "no delta reaches the consumer");
}

/// A reception the adapter cannot normalize is a fault in one reception, not
/// in the recording, so the reader counts it and reads on.
#[test]
fn a_refused_traffic_reception_is_counted_and_the_stream_continues() {
    let stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        unnormalizable_downlink(1_500),
        position_event(2_000, EVEN_POSITION),
    ]);

    let (engine, _deltas, outcome) = replay(&stream);
    let counts = outcome.expect("one refused reception does not stop the stream");

    assert_eq!(counts.receptions_consumed, 3);
    assert_eq!(counts.observations_produced, 2);
    assert_eq!(counts.receptions_skipped_not_traffic, 0);
    assert_eq!(counts.receptions_refused, 1);
    assert_counts_add_up(counts);
    assert_eq!(
        engine.track_count(),
        1,
        "the reception after the refused one still reached the engine"
    );
}

/// The two-phase decode exists so an unsupported schema is refused before its
/// payload is read. The payload here cannot deserialize into an event at all,
/// so a `Schema` failure proves the payload decoder never ran.
#[test]
fn an_unsupported_schema_version_is_refused_before_the_payload_is_decoded() {
    let stream = "{\"schema_version\":5,\"event\":\"this is not an event\"}\n";

    let (engine, _deltas, outcome) = replay(stream);

    match outcome {
        Err(ReplayError::Decode {
            line,
            source:
                ReceptionRecordDecodeError::Schema(ReceptionSchemaError::UnsupportedVersion {
                    supported,
                    actual,
                }),
        }) => {
            assert_eq!(line, 1);
            assert_eq!(supported, SUPPORTED_SCHEMA);
            assert_eq!(actual, 5);
        }
        other => panic!("expected a schema refusal before payload decoding, got {other:?}"),
    }
    assert_eq!(engine.track_count(), 0);
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

    let (engine, _deltas, outcome) = replay(&stream);

    match outcome {
        Err(ReplayError::Decode {
            source:
                ReceptionRecordDecodeError::Schema(ReceptionSchemaError::UnsupportedVersion {
                    actual,
                    ..
                }),
            ..
        }) => assert_eq!(actual, 0, "an absent version is not the current version"),
        other => panic!("expected an unversioned refusal, got {other:?}"),
    }
    assert_eq!(engine.track_count(), 0);
}

/// A line the decoder cannot read makes the rest of the recording
/// untrustworthy, so the reader stops there and names the line.
#[test]
fn a_malformed_line_stops_the_stream_and_names_its_line() {
    let mut stream = recorded(&[position_event(1_000, ODD_POSITION)]);
    stream.push_str("{ this is not JSON\n");
    stream.push_str(&recorded(&[position_event(2_000, EVEN_POSITION)]));

    let (engine, _deltas, outcome) = replay(&stream);

    match outcome {
        Err(ReplayError::Decode {
            line,
            source: ReceptionRecordDecodeError::Header { .. },
        }) => assert_eq!(line, 2),
        other => panic!("expected a header failure on line 2, got {other:?}"),
    }
    assert_eq!(
        engine.track_count(),
        1,
        "the reader stopped at the bad line, after the good one"
    );
}

/// A recording may carry blank lines from concatenation or an editor. They
/// are not receptions, so they must not inflate the tally.
#[test]
fn blank_lines_are_not_counted_as_receptions() {
    let mut stream = recorded(&[position_event(1_000, ODD_POSITION)]);
    stream.push_str("\n   \n");
    stream.push_str(&recorded(&[position_event(2_000, EVEN_POSITION)]));

    let (_engine, _deltas, outcome) = replay(&stream);
    let counts = outcome.expect("blank lines are not a failure");

    assert_eq!(counts.receptions_consumed, 2);
    assert_eq!(counts.observations_produced, 2);
    assert_counts_add_up(counts);
}

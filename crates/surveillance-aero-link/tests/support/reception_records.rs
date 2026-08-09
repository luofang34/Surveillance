//! Construction of recorded reception streams for the stream tests.
//!
//! Both stream test binaries include this file, and each one uses a part of
//! it, so an unused item here is expected rather than dead.

#![allow(dead_code)]

use aero_link::{
    AddressQualifier, Adsb1090Message, CorrectedUatFrame, ReceiverSource, ReceptionEvent,
    ReceptionRecord, SourceTransport, UatDecoder, UatDownlink, UatFrameClass, UatMessage,
    UatReception,
};
use surveillance_aero_link::NormalizeError;
use surveillance_aero_link::replay::{ReplayCounts, ReplayError, replay_records};
use surveillance_core::{EngineConfig, ProducerInstanceId, SurveillanceEngine, TrackDelta};

/// Even and odd halves of a published airborne position pair for one target.
pub const EVEN_POSITION: &str = "8D40621D58C382D690C8AC2863A7";
pub const ODD_POSITION: &str = "8D40621D58C386435CC412692AD6";
/// A published identification frame for a second, different target.
pub const OTHER_AIRCRAFT: &str = "8D4840D6202CC371C32CE0576098";

/// ICAO address carried by the position pair.
pub const POSITION_ADDRESS: u32 = 0x0040_621D;

/// Schema version this build supports, as `aero-link` defines it.
///
/// This is written out rather than imported, because a silent change of the
/// supported version upstream must fail these tests rather than follow them.
pub const SUPPORTED_SCHEMA: u16 = 6;

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

pub fn position_event(at: u64, hex: &str) -> ReceptionEvent {
    ReceptionEvent::adsb1090(at, decode(hex)).with_source(receiver())
}

/// A 978 MHz ground uplink: the band's weather and notice carrier, and the
/// reception the engine must never see.
pub fn ground_uplink(at: u64) -> ReceptionEvent {
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
pub fn unnormalizable_downlink(at: u64) -> ReceptionEvent {
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
pub fn recorded(events: &[ReceptionEvent]) -> String {
    let mut stream = String::new();
    for event in events {
        let record = ReceptionRecord::new(event.clone());
        stream.push_str(&serde_json::to_string(&record).expect("a record serializes"));
        stream.push('\n');
    }
    stream
}

/// Everything one replay produced, so a test can assert on all of it.
pub struct Replayed {
    pub engine: SurveillanceEngine,
    pub deltas: Vec<TrackDelta>,
    pub refusals: Vec<(u64, NormalizeError)>,
    pub outcome: Result<ReplayCounts, ReplayError>,
}

pub fn replay_with(config: EngineConfig, stream: &[u8]) -> Replayed {
    let mut engine = SurveillanceEngine::new(config, ProducerInstanceId::new(1));
    let mut deltas = Vec::new();
    let mut refusals = Vec::new();
    let outcome = replay_records(
        stream,
        &mut engine,
        |delta| deltas.push(delta.clone()),
        |line, error| refusals.push((line, *error)),
    );
    Replayed {
        engine,
        deltas,
        refusals,
        outcome,
    }
}

pub fn replay(stream: &str) -> Replayed {
    replay_with(EngineConfig::default(), stream.as_bytes())
}

/// Each reception has exactly one outcome, so the total is the sum of the
/// three ways a reception can end.
pub fn assert_counts_add_up(counts: ReplayCounts) {
    assert_eq!(
        counts.receptions_consumed,
        counts.observations_produced
            + counts.receptions_skipped_not_traffic
            + counts.receptions_refused,
        "every reception has exactly one outcome: {counts:?}"
    );
}

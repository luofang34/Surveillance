//! Umbrella re-export and end-to-end contract tests.

#![allow(clippy::expect_used, clippy::panic)]

use surveillance::{
    AddressNamespace, CURRENT_TRACK_SCHEMA_VERSION, EngineConfig, SurveillanceEngine, TrackDelta,
    TrackId, TrackKey, TrackRecord, TrackSnapshot, TrafficObservation,
};

use aero_link::{Adsb1090Message, ReceiverSource, ReceptionEvent, SourceTransport};

/// Even and odd halves of a published airborne position pair for one target.
const EVEN_POSITION: &str = "8D40621D58C382D690C8AC2863A7";
const ODD_POSITION: &str = "8D40621D58C386435CC412692AD6";

fn decode(hex: &str) -> Adsb1090Message {
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .map(|index| {
            u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("valid hex pair")
        })
        .collect();
    Adsb1090Message::decode(&bytes, None).expect("published frame decodes")
}

#[test]
fn the_umbrella_exports_the_schema_surface() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AB_CDEF);
    let record = TrackRecord::new(TrackDelta::Created(TrackSnapshot::new(
        TrackId::new(1),
        key,
        0,
    )));
    assert_eq!(record.schema_version(), CURRENT_TRACK_SCHEMA_VERSION);

    let serialized = serde_json::to_string(&record).expect("record serializes");
    let decoded: TrackRecord = serde_json::from_str(&serialized).expect("record decodes");
    assert_eq!(decoded, record);
}

#[test]
fn the_module_style_path_stays_available() {
    assert_eq!(surveillance::core::TrackId::new(2).get(), 2);
}

/// The caller pattern the project instructions specify: project a reception
/// to its traffic subset, normalize it, and feed the engine.
#[test]
fn receptions_become_tracks_through_the_documented_caller_pattern() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default());
    let source = ReceiverSource::new(1, SourceTransport::Usb);
    let mut deltas = Vec::new();

    for (at, hex) in [(1_000_u64, ODD_POSITION), (2_000, EVEN_POSITION)] {
        let event = ReceptionEvent::adsb1090(at, decode(hex)).with_source(source);
        if let Some(projection) = event.traffic() {
            let observation = TrafficObservation::normalize(&projection)
                .expect("a direct ADS-B frame normalizes");
            engine
                .ingest(observation, |delta| deltas.push(delta.clone()))
                .expect("ingest succeeds");
        }
    }

    assert_eq!(engine.track_count(), 1);
    let key = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);
    let snapshot = engine.snapshot(key).expect("the target became a track");
    let position = snapshot.position.as_ref().expect("the pair decoded");

    assert!(
        (position.value.latitude_deg - 52.257_202).abs() < 1e-4
            && (position.value.longitude_deg - 3.919_372).abs() < 1e-4,
        "decoded {:?}",
        position.value
    );
    assert_eq!(position.provenance.source, source);
    assert!(matches!(deltas.first(), Some(TrackDelta::Created(_))));

    let exported = TrackRecord::new(deltas.pop().expect("a delta was emitted"));
    let serialized = serde_json::to_string(&exported).expect("record serializes");
    let reparsed: TrackRecord = serde_json::from_str(&serialized).expect("record decodes");
    assert_eq!(reparsed, exported);
}

#[test]
fn a_non_traffic_reception_leaves_engine_state_untouched() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default());
    let event = ReceptionEvent::adsb1090(1_000, decode(EVEN_POSITION));

    // A weather consumer's uplink never projects into the traffic subset, so
    // the caller pattern simply never calls the engine for it.
    let uplink_projected = event.fis_b_messages().count();
    assert_eq!(uplink_projected, 0, "a 1090 frame carries no FIS-B");
    assert_eq!(engine.track_count(), 0);

    engine.advance_time(10_000_000, |_| {
        panic!("an untouched engine emits nothing");
    });
}

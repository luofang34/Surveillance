//! Umbrella re-export and source-neutrality contract tests.
//!
//! Nothing here imports `aero-link`. That is the point: a consumer that feeds
//! Surveillance from a panel, a network provider, a replay, or a simulator
//! must be able to do everything the engine offers without linking a radio
//! decoder.

#![allow(clippy::expect_used, clippy::panic)]

use surveillance::{
    AddressNamespace, CURRENT_TRACK_SCHEMA_VERSION, EngineConfig, PositionObservation, SourceRef,
    SourceTransport, SurveillanceEngine, TrackDelta, TrackId, TrackKey, TrackRecord, TrackSnapshot,
    TrafficLink, TrafficObservation, Wgs84Position,
};

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

/// A network provider or installed panel supplies an already-decoded report.
/// It has no radio, no band, and no receiver — and the engine tracks it all
/// the same.
#[test]
fn a_non_radio_source_can_drive_the_engine() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default());
    let provider = SourceRef::new(9, SourceTransport::Network);
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
    let mut deltas = Vec::new();

    for (at, latitude_deg) in [(1_000_u64, 40.0), (2_000, 40.001), (3_000, 40.002)] {
        let mut observation = TrafficObservation::new(at, provider, TrafficLink::Adsb1090, key);
        observation.position = Some(PositionObservation::Decoded(Wgs84Position {
            latitude_deg,
            longitude_deg: -74.0,
        }));
        observation.pressure_altitude_ft = Some(9_000);
        engine
            .ingest(observation, |delta| deltas.push(delta.clone()))
            .expect("ingest succeeds");
    }

    assert_eq!(engine.track_count(), 1);
    let snapshot = engine
        .snapshot(key)
        .expect("the provider report became a track");
    let position = snapshot.position.as_ref().expect("position applied");
    assert_eq!(position.value.latitude_deg, 40.002);
    assert_eq!(
        position.provenance.source, provider,
        "provenance names the provider, not a receiver"
    );
    assert!(matches!(deltas.first(), Some(TrackDelta::Created(_))));

    let exported = TrackRecord::new(deltas.pop().expect("a delta was emitted"));
    let serialized = serde_json::to_string(&exported).expect("record serializes");
    let reparsed: TrackRecord = serde_json::from_str(&serialized).expect("record decodes");
    assert_eq!(reparsed, exported);
}

/// Lifecycle is driven by the caller's clock regardless of what produced the
/// observations, so a non-radio source coasts and expires like any other.
#[test]
fn a_non_radio_track_coasts_and_expires_on_the_callers_clock() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default());
    let key = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);
    engine
        .ingest(
            TrafficObservation::new(
                0,
                SourceRef::new(4, SourceTransport::Replay),
                TrafficLink::Adsb1090,
                key,
            ),
            |_| {},
        )
        .expect("ingest succeeds");

    let coast_after = engine.config().freshness.coast_after_micros;
    let remove_after = engine.config().freshness.remove_after_micros;

    let mut coasting = Vec::new();
    engine.advance_time(coast_after + 1, |delta| coasting.push(delta.clone()));
    assert!(matches!(coasting.as_slice(), [TrackDelta::Coasting(_)]));

    let mut removed = Vec::new();
    engine.advance_time(remove_after + 1, |delta| removed.push(delta.clone()));
    assert!(matches!(removed.as_slice(), [TrackDelta::Removed { .. }]));
    assert_eq!(engine.track_count(), 0);
}

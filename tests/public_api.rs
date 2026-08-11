//! Umbrella re-export and source-neutrality contract tests.
//!
//! Nothing here imports `aero-link`. That is the point: a consumer that feeds
//! Surveillance from a panel, a network provider, a replay, or a simulator
//! must be able to do everything the engine offers without linking a radio
//! decoder.

#![allow(clippy::expect_used, clippy::panic)]

use surveillance::{
    AddressNamespace, Band, CURRENT_TRACK_SCHEMA_VERSION, Callsign, DeliveryPath, EngineConfig,
    FieldQuality, ObservationOrigin, ObservationTime, PositionObservation, ProducerInstanceId,
    SnapshotRevision, SourceRef, SurveillanceEngine, TimeBasis, TrackDelta, TrackId, TrackKey,
    TrackRecord, TrackSnapshot, TrackSnapshotHandle, TrafficObservation, Wgs84Position,
};

const PRODUCER: ProducerInstanceId = ProducerInstanceId::new(1);

#[test]
fn the_umbrella_exports_the_schema_surface() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AB_CDEF);
    let record = TrackRecord::new(TrackDelta::Created(TrackSnapshotHandle::new(
        PRODUCER,
        SnapshotRevision::new(1),
        TrackSnapshot::new(TrackId::new(1), key, 0),
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
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let provider = SourceRef::new(9, 0, DeliveryPath::NetworkProvider);
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
    let mut deltas = Vec::new();

    for (at, latitude_deg) in [(1_000_u64, 40.0), (2_000, 40.001), (3_000, 40.002)] {
        let mut observation = TrafficObservation::new(
            ObservationTime::local(at),
            provider,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            key,
        );
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
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let key = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);
    engine
        .ingest(
            TrafficObservation::new(
                ObservationTime::local(0),
                SourceRef::new(4, 0, DeliveryPath::Replay),
                ObservationOrigin::AdsbDirect {
                    band: Band::Adsb1090,
                },
                key,
            ),
            |_| {},
        )
        .expect("ingest succeeds");

    let coast_after = engine.config().freshness.coast_after_micros;
    let remove_after = engine.config().freshness.remove_after_micros;

    let mut coasting = Vec::new();
    engine
        .advance_time(coast_after + 1, |delta| coasting.push(delta.clone()))
        .expect("time advance succeeds");
    assert!(matches!(coasting.as_slice(), [TrackDelta::Coasting(_)]));

    let mut removed = Vec::new();
    engine
        .advance_time(remove_after + 1, |delta| removed.push(delta.clone()))
        .expect("time advance succeeds");
    assert!(matches!(removed.as_slice(), [TrackDelta::Removed { .. }]));
    assert_eq!(engine.track_count(), 0);
}

/// An installed panel reports a track it fused itself, over an aircraft bus.
/// Nothing about it resembles a radio, and the engine does not care.
#[test]
fn an_installed_panel_drives_the_engine_under_its_own_origin() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let panel = SourceRef::new(2, 0, DeliveryPath::InstalledAvionics);
    let key = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);

    let mut observation = TrafficObservation::new(
        ObservationTime::mapped(1_000_000, 900_000, 10_000),
        panel,
        ObservationOrigin::PanelFused,
        key,
    );
    observation.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 40.0,
        longitude_deg: -74.0,
    }));
    engine
        .ingest(observation, |_| {})
        .expect("the engine accepts a panel observation");

    let snapshot = engine
        .snapshot(key)
        .expect("the panel report became a track");
    let position = snapshot.position.as_ref().expect("position applied");
    assert_eq!(position.provenance.origin, ObservationOrigin::PanelFused);
    assert_eq!(
        position.provenance.source.delivery,
        DeliveryPath::InstalledAvionics
    );
    assert_eq!(
        position.time.observed_at_micros, 900_000,
        "the panel's own measurement time survives into provenance"
    );
}

/// A simulator is a delivery path like any other; the engine has no way to
/// tell, and no reason to.
#[test]
fn a_simulated_source_is_tracked_like_any_other() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let key = TrackKey::new(AddressNamespace::SelfAssigned, 0x0000_0001);
    let simulator = SourceRef::new(3, 0, DeliveryPath::Simulator);
    let mut observation = TrafficObservation::new(
        ObservationTime::local(1_000),
        simulator,
        ObservationOrigin::Replay,
        key,
    );
    observation.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 51.0,
        longitude_deg: 0.0,
    }));
    engine
        .ingest(observation, |_| {})
        .expect("the engine accepts a simulated observation");

    assert_eq!(engine.track_count(), 1);
    let snapshot = engine.snapshot(key).expect("track exists");
    let position = snapshot.position.as_ref().expect("position applied");
    assert_eq!(position.provenance.source, simulator);
    assert_eq!(position.provenance.origin, ObservationOrigin::Replay);
}

/// Track liveness follows reception time. Field age follows measurement time.
/// A delayed provider can keep a track active without making an old field
/// current.
#[test]
fn a_provider_with_honest_latency_keeps_the_track_active() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let provider = SourceRef::new(4, 0, DeliveryPath::NetworkProvider);
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
    let latency = 30_000_000;
    let coast_after = engine.config().freshness.coast_after_micros;

    let mut coasted = false;
    for step in 0..6_u64 {
        let received_at = 100_000_000 + step * 5_000_000;
        let mut observation = TrafficObservation::new(
            ObservationTime::mapped(received_at, received_at - latency, 0),
            provider,
            ObservationOrigin::ProviderFused,
            key,
        );
        observation.position = Some(PositionObservation::Decoded(Wgs84Position {
            latitude_deg: 40.0 + step as f64 * 0.001,
            longitude_deg: -74.0,
        }));
        engine.ingest(observation, |_| {}).expect("ingest succeeds");
        engine
            .advance_time(received_at, |delta| {
                if matches!(delta, TrackDelta::Coasting(_)) {
                    coasted = true;
                }
            })
            .expect("time advance succeeds");
    }

    assert!(latency > coast_after);
    assert!(!coasted, "current receptions keep the track active");
    let snapshot = engine.snapshot(key).expect("track stays available");
    assert_eq!(snapshot.phase, surveillance::TrackPhase::Active);
    assert_eq!(snapshot.last_observed_at_micros, 125_000_000 - latency);
}

/// Retention still applies to a source that will not say when it measured:
/// going silent expires it like any other. Whether repeated timeless reports
/// should be able to hold a track open indefinitely is a separate question,
/// and one this test deliberately does not answer.
#[test]
fn an_unknown_source_time_does_not_grant_indefinite_liveness() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let key = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);
    let remove_after = engine.config().freshness.remove_after_micros;

    engine
        .ingest(
            TrafficObservation::new(
                ObservationTime::unknown_source_time(1_000),
                SourceRef::new(5, 0, DeliveryPath::NetworkProvider),
                ObservationOrigin::Unknown,
                key,
            ),
            |_| {},
        )
        .expect("ingest succeeds");

    let mut removed = false;
    engine
        .advance_time(1_000 + remove_after + 1, |delta| {
            if matches!(delta, TrackDelta::Removed { .. }) {
                removed = true;
            }
        })
        .expect("time advance succeeds");
    assert!(removed, "retention still applies to a timeless source");
}

/// The headline case the source-neutral model exists for: one aircraft seen
/// three ways at once, with no source pretending to be something it is not,
/// and the strongest evidence winning each field on its own merits.
const RADIO: SourceRef = SourceRef::new(1, 0, DeliveryPath::LocalReceiver);
const PROVIDER: SourceRef = SourceRef::new(2, 1, DeliveryPath::NetworkProvider);
const PANEL: SourceRef = SourceRef::new(3, 0, DeliveryPath::InstalledAvionics);

/// One aircraft, reported three ways at the same instant.
fn three_sources(key: TrackKey) -> [TrafficObservation; 3] {
    let radio = RADIO;
    let provider = PROVIDER;
    let panel = PANEL;

    // The provider and the radio describe the *same instant*, so nothing but
    // origin can separate them. The provider even declares better quality, so
    // a fusion that ignored origin would prefer it and this test would fail.
    let measured_at = 1_000_000;
    let mut from_provider = TrafficObservation::new(
        ObservationTime::mapped(1_050_000, measured_at, 20_000),
        provider,
        ObservationOrigin::ProviderFused,
        key,
    );
    from_provider.quality = FieldQuality::new(Some(9), Some(9), Some(4), Some(3));
    from_provider.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 40.100,
        longitude_deg: -74.0,
    }));
    from_provider.pressure_altitude_ft = Some(30_000);

    // The panel contributes a callsign the radio has not heard.
    let mut from_panel = TrafficObservation::new(
        ObservationTime::mapped(1_100_000, 1_050_000, 5_000),
        panel,
        ObservationOrigin::PanelFused,
        key,
    );
    from_panel.callsign = Some(Callsign::new("DAL123"));

    // The radio hears the aircraft itself, at the same instant the provider
    // described, and declares no quality indicators at all.
    let mut from_radio = TrafficObservation::new(
        ObservationTime::local(measured_at),
        radio,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
        key,
    );
    from_radio.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 40.101,
        longitude_deg: -74.0,
    }));

    [from_provider, from_panel, from_radio]
}

#[test]
fn one_target_seen_through_a_radio_a_provider_and_a_panel() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);

    for observation in three_sources(key) {
        engine
            .ingest(observation, |_| {})
            .expect("every source reaches the same entry point");
    }

    assert_eq!(engine.track_count(), 1, "three sources, one aircraft");
    let snapshot = engine.snapshot(key).expect("track exists");

    let position = snapshot.position.as_ref().expect("position applied");
    assert_eq!(
        position.provenance.origin,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090
        },
        "the target's own transmission wins the position"
    );
    assert_eq!(
        position.provenance.source.delivery,
        DeliveryPath::LocalReceiver
    );

    let callsign = snapshot.callsign.as_ref().expect("callsign applied");
    assert_eq!(callsign.value.text, "DAL123");
    assert_eq!(
        callsign.provenance.origin,
        ObservationOrigin::PanelFused,
        "a field only the panel supplied stays attributed to the panel"
    );

    let altitude = snapshot
        .pressure_altitude_ft
        .as_ref()
        .expect("altitude applied");
    assert_eq!(
        altitude.provenance.source.epoch, 1,
        "the provider's incarnation survives into provenance"
    );
    assert_eq!(
        altitude.time.basis,
        TimeBasis::Mapped {
            uncertainty_micros: 20_000
        },
        "and so does how its time was established"
    );
}

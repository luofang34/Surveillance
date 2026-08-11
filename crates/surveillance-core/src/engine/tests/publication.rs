#![allow(clippy::expect_used, clippy::panic)]

use alloc::vec::Vec;

use crate::engine::tests::{ICAO, PRODUCER, advance, ingest, observation};
use crate::engine::{EngineConfig, IngestError, SnapshotPublicationError, SurveillanceEngine};
use crate::freshness::FreshnessPolicy;
use crate::observation::{EmitterCategory, ObservationOrigin, Wgs84Position};
use crate::track::{ProducerInstanceId, SnapshotRevision, TrackDelta, TrackKey, TrackPhase};

const OTHER: TrackKey = TrackKey::new(crate::track::AddressNamespace::Icao, 0x00AB_CDEF);

#[test]
fn two_engine_constructions_separate_restarted_track_identities() {
    let mut first =
        SurveillanceEngine::new(EngineConfig::default(), ProducerInstanceId::new(0x1001))
            .expect("default config is valid");
    let mut second =
        SurveillanceEngine::new(EngineConfig::default(), ProducerInstanceId::new(0x1002))
            .expect("default config is valid");
    ingest(
        &mut first,
        observation(0, ICAO, ObservationOrigin::ModeSReply),
    );
    ingest(
        &mut second,
        observation(0, ICAO, ObservationOrigin::ModeSReply),
    );
    let first_handle = first.snapshot(ICAO).expect("first track exists");
    let second_handle = second.snapshot(ICAO).expect("second track exists");

    assert_eq!(first_handle.id, second_handle.id);
    assert_ne!(
        first_handle.producer_instance_id(),
        second_handle.producer_instance_id()
    );
}

#[test]
fn a_retained_handle_does_not_change_when_the_engine_advances() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    ingest(
        &mut engine,
        observation(0, ICAO, ObservationOrigin::ModeSReply),
    );
    let retained = engine.snapshot(ICAO).expect("track exists");

    let deltas = advance(
        &mut engine,
        FreshnessPolicy::default().coast_after_micros + 1,
    );
    let current = engine.snapshot(ICAO).expect("track still exists");

    assert_eq!(retained.phase, TrackPhase::Active);
    assert_eq!(current.phase, TrackPhase::Coasting);
    assert_eq!(retained.snapshot_revision(), SnapshotRevision::new(1));
    assert_eq!(current.snapshot_revision(), SnapshotRevision::new(2));
    assert!(matches!(deltas.as_slice(), [TrackDelta::Coasting(_)]));
}

#[test]
fn revision_changes_only_when_exported_state_changes() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let observed = observation(1_000, ICAO, ObservationOrigin::ModeSReply);

    ingest(&mut engine, observed.clone());
    let created_revision = engine
        .snapshot(ICAO)
        .expect("track exists")
        .snapshot_revision();
    assert_eq!(created_revision, SnapshotRevision::new(1));

    assert!(ingest(&mut engine, observed).is_empty());
    engine
        .set_reference_position(Some(Wgs84Position {
            latitude_deg: 40.0,
            longitude_deg: -74.0,
        }))
        .expect("reference is valid");
    advance(&mut engine, 1_000);

    assert_eq!(
        engine
            .snapshot(ICAO)
            .expect("track exists")
            .snapshot_revision(),
        created_revision
    );
}

#[test]
fn a_freshness_refresh_revises_the_snapshot_without_a_delta() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let mut first = observation(1_000, ICAO, ObservationOrigin::ModeSReply);
    first.pressure_altitude_ft = Some(8_000);
    ingest(&mut engine, first.clone());

    first.time = crate::observation::ObservationTime::local(2_000);
    assert!(ingest(&mut engine, first).is_empty());

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert_eq!(snapshot.snapshot_revision(), SnapshotRevision::new(2));
    assert_eq!(snapshot.last_observed_at_micros, 2_000);
}

#[test]
fn revisions_order_published_states_of_one_subject() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let mut first = observation(1_000, ICAO, ObservationOrigin::ModeSReply);
    first.pressure_altitude_ft = Some(8_000);
    let mut other = observation(1_000, OTHER, ObservationOrigin::ModeSReply);
    other.pressure_altitude_ft = Some(9_000);
    ingest(&mut engine, first.clone());
    ingest(&mut engine, other);

    first.time = crate::observation::ObservationTime::local(2_000);
    first.pressure_altitude_ft = Some(8_100);
    ingest(&mut engine, first);

    assert_eq!(
        engine
            .snapshot(ICAO)
            .expect("first track exists")
            .snapshot_revision(),
        SnapshotRevision::new(2)
    );
    assert_eq!(
        engine
            .snapshot(OTHER)
            .expect("other track exists")
            .snapshot_revision(),
        SnapshotRevision::new(1)
    );
}

#[test]
fn an_ingest_does_not_publish_a_wrapped_revision() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    ingest(
        &mut engine,
        observation(0, ICAO, ObservationOrigin::ModeSReply),
    );
    let id = engine.snapshot(ICAO).expect("track exists").id;
    engine
        .tracks
        .get_mut(&id)
        .expect("track state exists")
        .snapshot_revision = SnapshotRevision::new(u64::MAX);

    let mut deltas = Vec::new();
    let error = engine
        .ingest(
            observation(1, ICAO, ObservationOrigin::ModeSReply),
            |delta| deltas.push(delta.clone()),
        )
        .expect_err("the producer instance must end before revision wrap");

    assert_eq!(
        error,
        IngestError::SnapshotPublication {
            source: SnapshotPublicationError::RevisionExhausted {
                producer_instance_id: PRODUCER,
                track_id: id,
                current_revision: SnapshotRevision::new(u64::MAX),
                required_publications: 1,
            },
        }
    );
    assert!(deltas.is_empty());
    let retained = engine.snapshot(ICAO).expect("last state stays available");
    assert_eq!(
        retained.snapshot_revision(),
        SnapshotRevision::new(u64::MAX)
    );
    assert_eq!(retained.last_observed_at_micros, 0);
}

#[test]
fn revision_exhaustion_does_not_add_a_classified_wire_alias() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    let classified = TrackKey::new(crate::AddressNamespace::SelfAssigned, 0x00AB_CDEF);
    ingest(
        &mut engine,
        observation(0, classified, ObservationOrigin::ModeSReply),
    );
    let id = engine.snapshot(classified).expect("track exists").id;
    engine
        .tracks
        .get_mut(&id)
        .expect("track state exists")
        .snapshot_revision = SnapshotRevision::new(u64::MAX);
    let wire = TrackKey::new(crate::AddressNamespace::AdsbNonIcao, 0x00AB_CDEF);
    let mut observed = observation(1, wire, ObservationOrigin::ModeSReply);
    observed.emitter_category = Some(EmitterCategory::LightAircraft);

    let error = engine
        .ingest(observed, |_| panic!("a failed ingest must emit nothing"))
        .expect_err("the producer instance is exhausted");

    assert!(matches!(error, IngestError::SnapshotPublication { .. }));
    assert_eq!(engine.snapshot(wire), None);
    let retained = engine.snapshot(classified).expect("original track remains");
    assert!(!retained.identities.contains(wire));
    assert_eq!(retained.last_observed_at_micros, 0);
}

#[test]
fn time_advance_does_not_publish_a_wrapped_revision() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    ingest(
        &mut engine,
        observation(0, ICAO, ObservationOrigin::ModeSReply),
    );
    let id = engine.snapshot(ICAO).expect("track exists").id;
    engine
        .tracks
        .get_mut(&id)
        .expect("track state exists")
        .snapshot_revision = SnapshotRevision::new(u64::MAX);

    let mut deltas = Vec::new();
    let error = engine
        .advance_time(FreshnessPolicy::default().coast_after_micros + 1, |delta| {
            deltas.push(delta.clone())
        })
        .expect_err("the producer instance must end before revision wrap");

    assert_eq!(
        error,
        SnapshotPublicationError::RevisionExhausted {
            producer_instance_id: PRODUCER,
            track_id: id,
            current_revision: SnapshotRevision::new(u64::MAX),
            required_publications: 1,
        }
    );
    assert!(deltas.is_empty());
    let retained = engine.snapshot(ICAO).expect("last state stays available");
    assert_eq!(retained.phase, TrackPhase::Active);
    assert_eq!(
        retained.snapshot_revision(),
        SnapshotRevision::new(u64::MAX)
    );
}

#[test]
fn a_removal_gets_the_subjects_next_revision() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
        .expect("default config is valid");
    ingest(
        &mut engine,
        observation(0, ICAO, ObservationOrigin::ModeSReply),
    );

    let removed = advance(
        &mut engine,
        FreshnessPolicy::default().remove_after_micros + 1,
    );

    assert!(matches!(removed.as_slice(), [TrackDelta::Removed { .. }]));
    assert_eq!(removed[0].producer_instance_id(), PRODUCER);
    assert_eq!(removed[0].snapshot_revision(), SnapshotRevision::new(2));
}

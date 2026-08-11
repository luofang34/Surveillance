#![allow(clippy::expect_used, clippy::panic)]

use alloc::vec::Vec;

use crate::source::{Band, DeliveryPath, SourceRef};

use crate::engine::{EngineConfig, IngestError, SurveillanceEngine};
use crate::observation::{
    Callsign, ObservationOrigin, ObservationTime, Squawk, TrafficObservation,
};
use crate::track::{AddressNamespace, ProducerInstanceId, TrackDelta, TrackId, TrackKey};

mod association;
mod classification;
mod lifecycle;
mod position;
mod publication;

pub(super) const ICAO: TrackKey = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
pub(super) const OTHER_ICAO: TrackKey = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);
pub(super) const PRODUCER: ProducerInstanceId = ProducerInstanceId::new(1);

pub(super) fn receiver(id: u32) -> SourceRef {
    SourceRef::new(id, 0, DeliveryPath::LocalReceiver)
}

pub(super) fn observation(at: u64, key: TrackKey, origin: ObservationOrigin) -> TrafficObservation {
    TrafficObservation::new(ObservationTime::local(at), receiver(1), origin, key)
}

pub(super) fn engine() -> SurveillanceEngine {
    SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
}

pub(super) fn ingest(
    engine: &mut SurveillanceEngine,
    observation: TrafficObservation,
) -> Vec<TrackDelta> {
    let mut deltas = Vec::new();
    engine
        .ingest(observation, |delta| deltas.push(delta.clone()))
        .expect("ingest succeeds");
    deltas
}

pub(super) fn advance(engine: &mut SurveillanceEngine, now_micros: u64) -> Vec<TrackDelta> {
    let mut deltas = Vec::new();
    engine
        .advance_time(now_micros, |delta| deltas.push(delta.clone()))
        .expect("time advance succeeds");
    deltas
}

#[test]
fn a_new_engine_maintains_no_tracks() {
    let engine = engine();
    assert_eq!(engine.track_count(), 0);
    assert_eq!(engine.snapshots().count(), 0);
    assert_eq!(engine.snapshot(ICAO), None);
    assert_eq!(engine.reference_position(), None);
}

#[test]
fn a_first_observation_creates_a_position_absent_track() {
    let mut engine = engine();
    let mut observed = observation(1_000, ICAO, ObservationOrigin::ModeSReply);
    observed.pressure_altitude_ft = Some(12_500);

    let deltas = ingest(&mut engine, observed);
    assert_eq!(deltas.len(), 1);
    let TrackDelta::Created(snapshot) = &deltas[0] else {
        panic!("first observation must create, got {:?}", deltas[0]);
    };

    assert_eq!(snapshot.key, ICAO);
    assert_eq!(snapshot.position, None);
    assert_eq!(
        snapshot
            .pressure_altitude_ft
            .as_ref()
            .expect("altitude applied")
            .value,
        12_500
    );
    assert_eq!(snapshot.created_at_micros, 1_000);
    assert_eq!(snapshot.last_observed_at_micros, 1_000);
    assert_eq!(engine.track_count(), 1);
}

#[test]
fn a_changed_field_updates_and_a_duplicate_stays_silent() {
    let mut engine = engine();
    let mut first = observation(
        1_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    first.callsign = Some(Callsign::new("DAL123"));
    assert_eq!(ingest(&mut engine, first.clone()).len(), 1);

    let mut repeated = first.clone();
    repeated.time = ObservationTime::local(2_000);
    assert!(
        ingest(&mut engine, repeated).is_empty(),
        "an unchanged value must not emit a delta"
    );

    let mut renamed = first;
    renamed.time = ObservationTime::local(3_000);
    renamed.callsign = Some(Callsign::new("DAL456"));
    let deltas = ingest(&mut engine, renamed);
    assert_eq!(deltas.len(), 1);
    assert!(matches!(deltas[0], TrackDelta::Updated(_)));
}

#[test]
fn a_duplicate_from_another_receiver_publishes_the_source_handover() {
    let mut engine = engine();
    let mut first = observation(
        1_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    first.pressure_altitude_ft = Some(9_000);
    ingest(&mut engine, first.clone());

    let mut second = first;
    second.time = ObservationTime::local(1_500);
    second.source = receiver(2);
    let deltas = ingest(&mut engine, second);
    assert!(matches!(deltas.as_slice(), [TrackDelta::Updated(_)]));

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    let altitude = snapshot
        .pressure_altitude_ft
        .as_ref()
        .expect("altitude applied");
    assert_eq!(altitude.observed_at_micros(), 1_500);
    assert_eq!(altitude.provenance.source, receiver(2));
    assert_eq!(snapshot.last_observed_at_micros, 1_500);
}

#[test]
fn a_late_older_observation_does_not_overwrite_a_newer_value() {
    let mut engine = engine();
    let mut newer = observation(
        5_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    newer.pressure_altitude_ft = Some(9_000);
    ingest(&mut engine, newer);

    let mut older = observation(
        4_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    older.pressure_altitude_ft = Some(3_000);
    assert!(ingest(&mut engine, older).is_empty());

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert_eq!(
        snapshot
            .pressure_altitude_ft
            .as_ref()
            .expect("altitude applied")
            .value,
        9_000
    );
    assert_eq!(
        snapshot.last_observed_at_micros, 5_000,
        "a late observation must not rewind track liveness"
    );
}

#[test]
fn equal_addresses_in_different_namespaces_stay_distinct_tracks() {
    let mut engine = engine();
    let track_file = TrackKey::new(AddressNamespace::TisbTrackFile, ICAO.address);
    let vehicle = TrackKey::new(AddressNamespace::SurfaceVehicle, ICAO.address);

    ingest(
        &mut engine,
        observation(
            1_000,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );
    ingest(
        &mut engine,
        observation(
            1_100,
            track_file,
            ObservationOrigin::Tisb {
                band: crate::source::Band::Uat978,
            },
        ),
    );
    ingest(
        &mut engine,
        observation(
            1_200,
            vehicle,
            ObservationOrigin::AdsbDirect { band: Band::Uat978 },
        ),
    );

    assert_eq!(engine.track_count(), 3);
    let ids: Vec<TrackId> = engine.snapshots().map(|snapshot| snapshot.id).collect();
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
}

#[test]
fn interleaved_aircraft_and_receivers_do_not_cross_talk() {
    let mut engine = engine();
    let mut first = observation(
        1_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    first.callsign = Some(Callsign::new("AAL1"));
    let mut second = observation(
        1_100,
        OTHER_ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    second.source = receiver(2);
    second.callsign = Some(Callsign::new("UAL2"));

    ingest(&mut engine, first);
    ingest(&mut engine, second);

    let mut third = observation(
        1_200,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    third.source = receiver(2);
    third.squawk = Some(Squawk::try_from(1200).expect("valid code"));
    ingest(&mut engine, third);

    let one = engine.snapshot(ICAO).expect("first track exists");
    let two = engine.snapshot(OTHER_ICAO).expect("second track exists");
    assert_eq!(
        one.callsign.as_ref().expect("callsign applied").value.text,
        "AAL1"
    );
    assert_eq!(
        two.callsign.as_ref().expect("callsign applied").value.text,
        "UAL2"
    );
    assert!(one.squawk.is_some());
    assert!(two.squawk.is_none(), "fields must not leak between tracks");
}

#[test]
fn unsupplied_fields_stay_absent() {
    let mut engine = engine();
    let mut observed = observation(1_000, ICAO, ObservationOrigin::ModeSReply);
    observed.pressure_altitude_ft = Some(9_000);
    ingest(&mut engine, observed);

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert!(snapshot.position.is_none());
    assert!(snapshot.geometric_altitude_ft.is_none());
    assert!(snapshot.velocity.is_none());
    assert!(snapshot.callsign.is_none());
    assert!(snapshot.squawk.is_none());
    assert!(snapshot.air_ground.is_none());
    assert!(snapshot.emergency.is_none());
}

#[test]
fn track_capacity_is_enforced_without_disturbing_existing_tracks() {
    let mut engine = SurveillanceEngine::new(
        EngineConfig {
            max_tracks: Some(1),
            ..EngineConfig::default()
        },
        PRODUCER,
    );

    ingest(
        &mut engine,
        observation(
            1_000,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );
    let outcome = engine.ingest(
        observation(
            1_100,
            OTHER_ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
        |_| panic!("a rejected observation must emit nothing"),
    );

    assert_eq!(
        outcome,
        Err(IngestError::TrackCapacityExceeded { capacity: 1 })
    );
    assert_eq!(engine.track_count(), 1);
    assert!(engine.snapshot(ICAO).is_some());

    ingest(
        &mut engine,
        observation(
            1_200,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );
    assert_eq!(engine.track_count(), 1);
}

#[test]
fn a_wrapped_track_identity_cannot_replace_a_live_track() {
    let mut engine = engine();
    ingest(
        &mut engine,
        observation(1_000, ICAO, ObservationOrigin::ModeSReply),
    );
    engine.next_id = 0;

    let outcome = engine.ingest(
        observation(2_000, OTHER_ICAO, ObservationOrigin::ModeSReply),
        |_| panic!("a rejected observation must emit nothing"),
    );

    assert_eq!(
        outcome,
        Err(IngestError::TrackIdentityExhausted {
            id: TrackId::new(0),
        })
    );
    assert_eq!(engine.track_count(), 1);
    assert!(engine.snapshot(ICAO).is_some());
    assert_eq!(engine.snapshot(OTHER_ICAO), None);
}

#[test]
fn a_track_keeps_one_identity_across_its_life() {
    let mut engine = engine();
    let created = ingest(
        &mut engine,
        observation(
            1_000,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );
    let id = created[0].id();

    let mut later = observation(
        2_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    later.squawk = Some(Squawk::try_from(4321).expect("valid code"));
    let updated = ingest(&mut engine, later);

    assert_eq!(updated[0].id(), id);
    assert_eq!(engine.snapshot_by_id(id).expect("track exists").key, ICAO);
}

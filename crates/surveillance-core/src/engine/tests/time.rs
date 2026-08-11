#![allow(clippy::expect_used, clippy::panic)]

use crate::engine::tests::{ICAO, advance, engine, ingest};
use crate::freshness::FreshnessPolicy;
use crate::observation::{ObservationOrigin, ObservationTime, TrafficObservation};
use crate::source::SourceRef;
use crate::track::{TrackDelta, TrackPhase};

#[test]
fn a_first_delayed_observation_ages_liveness_from_reception() {
    let received_at_micros = 100_000_000;
    let observed_at_micros = 1_000_000;
    let mut engine = engine();
    let observed = TrafficObservation::new(
        ObservationTime::mapped(received_at_micros, observed_at_micros, 0),
        SourceRef::default(),
        ObservationOrigin::ProviderFused,
        ICAO,
    );

    let created = ingest(&mut engine, observed);
    let TrackDelta::Created(snapshot) = &created[0] else {
        panic!("the first observation must create a track");
    };
    assert_eq!(snapshot.created_at_micros, received_at_micros);
    assert_eq!(snapshot.last_observed_at_micros, observed_at_micros);

    let coast_after_micros = FreshnessPolicy::default().coast_after_micros;
    assert!(advance(&mut engine, received_at_micros + coast_after_micros).is_empty());
    assert!(matches!(
        advance(&mut engine, received_at_micros + coast_after_micros + 1).as_slice(),
        [TrackDelta::Coasting(_)]
    ));
}

#[test]
fn unknown_source_time_uses_ingress_for_liveness() {
    let received_at_micros = 5_000;
    let mut engine = engine();
    let observed = TrafficObservation::new(
        ObservationTime::unknown_source_time(received_at_micros),
        SourceRef::default(),
        ObservationOrigin::Unknown,
        ICAO,
    );
    ingest(&mut engine, observed);

    let coast_after_micros = FreshnessPolicy::default().coast_after_micros;
    advance(&mut engine, received_at_micros + coast_after_micros + 1);
    assert_eq!(
        engine.snapshot(ICAO).expect("track exists").phase,
        TrackPhase::Coasting
    );
}

#[test]
fn an_older_reception_does_not_reactivate_a_coasting_track() {
    let mut engine = engine();
    ingest(
        &mut engine,
        TrafficObservation::new(
            ObservationTime::local(1_000),
            SourceRef::default(),
            ObservationOrigin::ModeSReply,
            ICAO,
        ),
    );
    advance(
        &mut engine,
        1_000 + FreshnessPolicy::default().coast_after_micros + 1,
    );

    ingest(
        &mut engine,
        TrafficObservation::new(
            ObservationTime::local(999),
            SourceRef::default(),
            ObservationOrigin::ModeSReply,
            ICAO,
        ),
    );

    assert_eq!(
        engine.snapshot(ICAO).expect("track exists").phase,
        TrackPhase::Coasting
    );
}

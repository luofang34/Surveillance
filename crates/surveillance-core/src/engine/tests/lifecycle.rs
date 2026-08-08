#![allow(clippy::expect_used, clippy::panic)]

use alloc::vec::Vec;

use crate::engine::tests::{ICAO, OTHER_ICAO, advance, engine, ingest, observation};
use crate::engine::{EngineConfig, SurveillanceEngine};
use crate::freshness::FreshnessPolicy;
use crate::observation::TrafficLink;
use crate::track::{AddressNamespace, RemovalReason, TrackDelta, TrackKey, TrackPhase};

/// Windows of the default policy these tests drive time against.
fn coast_at() -> u64 {
    FreshnessPolicy::default().coast_after_micros
}

fn remove_at() -> u64 {
    FreshnessPolicy::default().remove_after_micros
}

#[test]
fn a_track_coasts_once_and_then_stays_quiet() {
    let mut engine = engine();
    ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));

    assert!(
        advance(&mut engine, coast_at()).is_empty(),
        "a track at the coasting boundary is still active"
    );

    let deltas = advance(&mut engine, coast_at() + 1);
    assert_eq!(deltas.len(), 1);
    let TrackDelta::Coasting(snapshot) = &deltas[0] else {
        panic!("expected a coasting delta, got {:?}", deltas[0]);
    };
    assert_eq!(snapshot.phase, TrackPhase::Coasting);

    assert!(
        advance(&mut engine, coast_at() + 2).is_empty(),
        "coasting must emit one transition, not one delta per call"
    );
}

#[test]
fn a_fresh_observation_returns_a_coasting_track_to_active() {
    let mut engine = engine();
    ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));
    advance(&mut engine, coast_at() + 1);

    let reacquired = coast_at() + 2;
    let deltas = ingest(
        &mut engine,
        observation(reacquired, ICAO, TrafficLink::Adsb1090),
    );

    assert_eq!(deltas.len(), 1);
    let TrackDelta::Updated(snapshot) = &deltas[0] else {
        panic!("reacquisition must update, got {:?}", deltas[0]);
    };
    assert_eq!(snapshot.phase, TrackPhase::Active);
}

#[test]
fn a_track_is_removed_after_its_retention_window() {
    let mut engine = engine();
    ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));
    advance(&mut engine, coast_at() + 1);

    let deltas = advance(&mut engine, remove_at() + 1);
    assert_eq!(deltas.len(), 1);
    let TrackDelta::Removed { key, reason, .. } = &deltas[0] else {
        panic!("expected a removal, got {:?}", deltas[0]);
    };
    assert_eq!(*key, ICAO);
    assert_eq!(*reason, RemovalReason::Expired);
    assert_eq!(engine.track_count(), 0);
    assert_eq!(engine.snapshot(ICAO), None);
}

#[test]
fn removal_releases_the_key_for_a_new_track() {
    let mut engine = engine();
    let created = ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));
    let first_id = created[0].id();
    advance(&mut engine, remove_at() + 1);

    let recreated = ingest(
        &mut engine,
        observation(remove_at() + 2, ICAO, TrafficLink::Adsb1090),
    );
    assert!(matches!(recreated[0], TrackDelta::Created(_)));
    assert_ne!(
        recreated[0].id(),
        first_id,
        "a removed track's identity is not reused"
    );
    assert_eq!(engine.track_count(), 1);
}

#[test]
fn a_position_absent_track_coasts_and_expires_like_any_other() {
    let mut engine = engine();
    let mut observed = observation(0, ICAO, TrafficLink::ModeS);
    observed.pressure_altitude_ft = Some(7_500);
    ingest(&mut engine, observed);

    assert!(matches!(
        advance(&mut engine, coast_at() + 1).as_slice(),
        [TrackDelta::Coasting(_)]
    ));
    assert!(matches!(
        advance(&mut engine, remove_at() + 1).as_slice(),
        [TrackDelta::Removed { .. }]
    ));
}

#[test]
fn time_running_backwards_transitions_nothing() {
    let mut engine = engine();
    ingest(
        &mut engine,
        observation(1_000_000, ICAO, TrafficLink::Adsb1090),
    );

    assert!(advance(&mut engine, 0).is_empty());
    assert!(advance(&mut engine, 500).is_empty());
    assert_eq!(engine.track_count(), 1);
    assert_eq!(
        engine.snapshot(ICAO).expect("track exists").phase,
        TrackPhase::Active
    );
}

#[test]
fn each_field_ages_on_its_own_window() {
    let mut engine = SurveillanceEngine::new(EngineConfig {
        freshness: FreshnessPolicy {
            position_fresh_micros: 1_000,
            identity_fresh_micros: 100_000,
            ..FreshnessPolicy::default()
        },
        ..EngineConfig::default()
    });

    let mut positioned = observation(0, ICAO, TrafficLink::Adsb1090);
    positioned.pressure_altitude_ft = Some(10_000);
    ingest(&mut engine, positioned);

    let mut identified = observation(50_000, ICAO, TrafficLink::Adsb1090);
    identified.callsign = Some(crate::observation::Callsign::new("DAL9"));
    ingest(&mut engine, identified);

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert_eq!(
        snapshot
            .pressure_altitude_ft
            .as_ref()
            .expect("altitude applied")
            .observed_at_micros,
        0,
        "a callsign update must not refresh an older altitude"
    );
    assert!(
        !snapshot
            .pressure_altitude_ft
            .as_ref()
            .expect("altitude applied")
            .is_fresh(50_000, 1_000)
    );
    assert!(
        snapshot
            .callsign
            .as_ref()
            .expect("callsign applied")
            .is_fresh(50_000, 100_000)
    );
}

#[test]
fn tracks_expire_in_track_identity_order() {
    let mut engine = engine();
    ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));
    ingest(
        &mut engine,
        observation(0, OTHER_ICAO, TrafficLink::Adsb1090),
    );

    let removed: Vec<u64> = advance(&mut engine, remove_at() + 1)
        .iter()
        .filter(|delta| matches!(delta, TrackDelta::Removed { .. }))
        .map(|delta| delta.id().get())
        .collect();
    assert_eq!(removed, [0, 1]);
}

#[test]
fn an_identical_replay_produces_an_identical_delta_stream() {
    fn run() -> Vec<TrackDelta> {
        let mut engine = engine();
        let mut deltas = Vec::new();
        for (at, key) in [(0, ICAO), (1_000, OTHER_ICAO), (2_000, ICAO)] {
            let mut observed = observation(at, key, TrafficLink::Adsb1090);
            observed.pressure_altitude_ft = Some(10_000 + at as i32);
            engine
                .ingest(observed, |delta| deltas.push(delta.clone()))
                .expect("ingest succeeds");
        }
        for now in [20_000_000, 100_000_000] {
            engine.advance_time(now, |delta| deltas.push(delta.clone()));
        }
        deltas
    }

    assert_eq!(run(), run());
}

#[test]
fn ownship_identities_label_without_suppressing() {
    let mut engine = engine();
    ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));
    ingest(
        &mut engine,
        observation(0, OTHER_ICAO, TrafficLink::Adsb1090),
    );

    let mut deltas = Vec::new();
    engine.set_ownship_identities(&[ICAO], |delta| deltas.push(delta.clone()));

    assert_eq!(deltas.len(), 1, "only the matching track changes");
    assert!(
        engine
            .snapshot(ICAO)
            .expect("ownship track exists")
            .ownship_shadow
    );
    assert!(
        !engine
            .snapshot(OTHER_ICAO)
            .expect("intruder track exists")
            .ownship_shadow
    );
    assert_eq!(
        engine.track_count(),
        2,
        "labeling never removes a track from the engine"
    );
}

#[test]
fn replacing_the_ownship_set_relabels_in_both_directions() {
    let mut engine = engine();
    ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));
    ingest(
        &mut engine,
        observation(0, OTHER_ICAO, TrafficLink::Adsb1090),
    );
    engine.set_ownship_identities(&[ICAO], |_| {});

    let mut deltas = Vec::new();
    engine.set_ownship_identities(&[OTHER_ICAO], |delta| deltas.push(delta.clone()));

    assert_eq!(deltas.len(), 2, "one track is labeled and one un-labeled");
    assert!(!engine.snapshot(ICAO).expect("track exists").ownship_shadow);
    assert!(
        engine
            .snapshot(OTHER_ICAO)
            .expect("track exists")
            .ownship_shadow
    );

    let mut cleared = Vec::new();
    engine.set_ownship_identities(&[], |delta| cleared.push(delta.clone()));
    assert_eq!(cleared.len(), 1);
    assert!(
        engine
            .snapshot(OTHER_ICAO)
            .expect("track exists")
            .ownship_shadow
            .eq(&false)
    );
}

#[test]
fn a_track_acquired_after_the_ownship_set_is_labeled_on_creation() {
    let mut engine = engine();
    engine.set_ownship_identities(&[ICAO], |_| {});

    let deltas = ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));
    let TrackDelta::Created(snapshot) = &deltas[0] else {
        panic!("expected a creation, got {:?}", deltas[0]);
    };
    assert!(snapshot.ownship_shadow);
}

#[test]
fn an_unrelated_ownship_set_leaves_every_track_unlabeled() {
    let mut engine = engine();
    ingest(&mut engine, observation(0, ICAO, TrafficLink::Adsb1090));

    let stranger = TrackKey::new(AddressNamespace::SelfAssigned, 0x0000_0001);
    let mut deltas = Vec::new();
    engine.set_ownship_identities(&[stranger], |delta| deltas.push(delta.clone()));

    assert!(deltas.is_empty());
    assert!(!engine.snapshot(ICAO).expect("track exists").ownship_shadow);
}

//! Caller-supplied ownship identity behavior.

#![allow(clippy::expect_used, clippy::panic)]

use alloc::vec::Vec;

use crate::engine::tests::{ICAO, OTHER_ICAO, engine, ingest, observation};
use crate::observation::ObservationOrigin;
use crate::source::Band;
use crate::track::{AddressNamespace, TrackDelta, TrackKey};

#[test]
fn ownship_identities_label_without_suppressing() {
    let mut engine = engine();
    ingest(
        &mut engine,
        observation(
            0,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );
    ingest(
        &mut engine,
        observation(
            0,
            OTHER_ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );

    let mut deltas = Vec::new();
    engine
        .set_ownship_identities(&[ICAO], |delta| deltas.push(delta.clone()))
        .expect("ownship update succeeds");

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
    ingest(
        &mut engine,
        observation(
            0,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );
    ingest(
        &mut engine,
        observation(
            0,
            OTHER_ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );
    engine
        .set_ownship_identities(&[ICAO], |_| {})
        .expect("ownship update succeeds");

    let mut deltas = Vec::new();
    engine
        .set_ownship_identities(&[OTHER_ICAO], |delta| deltas.push(delta.clone()))
        .expect("ownship update succeeds");

    assert_eq!(deltas.len(), 2, "one track is labeled and one un-labeled");
    assert!(!engine.snapshot(ICAO).expect("track exists").ownship_shadow);
    assert!(
        engine
            .snapshot(OTHER_ICAO)
            .expect("track exists")
            .ownship_shadow
    );

    let mut cleared = Vec::new();
    engine
        .set_ownship_identities(&[], |delta| cleared.push(delta.clone()))
        .expect("ownship update succeeds");
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
    engine
        .set_ownship_identities(&[ICAO], |_| {})
        .expect("ownship update succeeds");

    let deltas = ingest(
        &mut engine,
        observation(
            0,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );
    let TrackDelta::Created(snapshot) = &deltas[0] else {
        panic!("expected a creation, got {:?}", deltas[0]);
    };
    assert!(snapshot.ownship_shadow);
}

#[test]
fn an_unrelated_ownship_set_leaves_every_track_unlabeled() {
    let mut engine = engine();
    ingest(
        &mut engine,
        observation(
            0,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );

    let stranger = TrackKey::new(AddressNamespace::SelfAssigned, 0x0000_0001);
    let mut deltas = Vec::new();
    engine
        .set_ownship_identities(&[stranger], |delta| deltas.push(delta.clone()))
        .expect("ownship update succeeds");

    assert!(deltas.is_empty());
    assert!(!engine.snapshot(ICAO).expect("track exists").ownship_shadow);
}

#![allow(clippy::expect_used, clippy::panic)]

use crate::engine::{EngineConfig, SurveillanceEngine};
use crate::track::{AddressNamespace, TrackKey};

#[test]
fn a_new_engine_maintains_no_tracks() {
    let engine = SurveillanceEngine::new(EngineConfig::default());
    assert_eq!(engine.track_count(), 0);
    assert_eq!(engine.snapshots().count(), 0);
    assert_eq!(
        engine.snapshot(TrackKey::new(AddressNamespace::Icao, 1)),
        None
    );
    assert_eq!(engine.config(), &EngineConfig::default());
}

#[test]
fn ownship_identities_replace_wholesale() {
    let mut engine = SurveillanceEngine::new(EngineConfig::default());
    let ownship = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
    let previous = TrackKey::new(AddressNamespace::SelfAssigned, 0x0099_0011);

    engine.set_ownship_identities(&[previous, ownship]);
    assert_eq!(engine.ownship_identities(), &[previous, ownship]);

    engine.set_ownship_identities(&[ownship]);
    assert_eq!(engine.ownship_identities(), &[ownship]);
}

#![allow(clippy::expect_used, clippy::panic)]

use crate::identity::IdentitySet;
use crate::track::{AddressNamespace, TrackKey};

const ICAO: TrackKey = TrackKey::new(AddressNamespace::Icao, 0x00AB_CDEF);
const TRACK_FILE: TrackKey = TrackKey::new(AddressNamespace::TisbTrackFile, 0x0000_0042);

#[test]
fn a_new_set_contains_only_the_primary() {
    let set = IdentitySet::new(ICAO);
    assert_eq!(set.primary(), ICAO);
    assert_eq!(set.associated(), &[]);
    assert!(set.contains(ICAO));
    assert!(!set.contains(TRACK_FILE));
}

#[test]
fn association_is_duplicate_free() {
    let mut set = IdentitySet::new(ICAO);
    set.associate(TRACK_FILE);
    set.associate(TRACK_FILE);
    set.associate(ICAO);
    assert_eq!(set.associated(), &[TRACK_FILE]);
    assert!(set.contains(TRACK_FILE));
}

#[test]
fn promotion_re_keys_without_losing_identities() {
    let mut set = IdentitySet::new(TRACK_FILE);
    set.associate(ICAO);

    set.promote(ICAO);
    assert_eq!(set.primary(), ICAO);
    assert_eq!(set.associated(), &[TRACK_FILE]);

    set.promote(TRACK_FILE);
    assert_eq!(set.primary(), TRACK_FILE);
    assert_eq!(set.associated(), &[ICAO]);
}

#[test]
fn promoting_the_current_primary_changes_nothing() {
    let mut set = IdentitySet::new(ICAO);
    set.associate(TRACK_FILE);
    set.promote(ICAO);
    assert_eq!(set.primary(), ICAO);
    assert_eq!(set.associated(), &[TRACK_FILE]);
}

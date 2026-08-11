#![allow(clippy::expect_used, clippy::panic)]

use crate::track::{
    AddressNamespace, PositionStatus, ProducerInstanceId, RemovalReason, SnapshotRevision,
    TrackDelta, TrackId, TrackKey, TrackPhase, TrackSnapshot, TrackSnapshotHandle,
};

#[test]
fn equal_addresses_in_different_namespaces_are_distinct_keys() {
    let icao = TrackKey::new(AddressNamespace::Icao, 0x0012_3456);
    let self_assigned = TrackKey::new(AddressNamespace::SelfAssigned, 0x0012_3456);
    let track_file = TrackKey::new(AddressNamespace::TisbTrackFile, 0x0012_3456);
    let vehicle = TrackKey::new(AddressNamespace::SurfaceVehicle, 0x0012_3456);

    assert_ne!(icao, self_assigned);
    assert_ne!(icao, track_file);
    assert_ne!(icao, vehicle);
    assert_ne!(self_assigned, track_file);
    assert_ne!(self_assigned, vehicle);
    assert_ne!(track_file, vehicle);
}

#[test]
fn a_new_snapshot_is_active_and_position_absent() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00A1_B2C3);
    let snapshot = TrackSnapshot::new(TrackId::new(7), key, 42_000);

    assert_eq!(snapshot.id, TrackId::new(7));
    assert_eq!(snapshot.key, key);
    assert_eq!(snapshot.identities.primary(), key);
    assert_eq!(snapshot.phase, TrackPhase::Active);
    assert!(!snapshot.ownship_shadow);
    assert_eq!(snapshot.position, None);
    assert_eq!(snapshot.position_status, PositionStatus::Unavailable);
    assert_eq!(snapshot.created_at_micros, 42_000);
    assert_eq!(snapshot.last_observed_at_micros, 42_000);
}

#[test]
fn every_delta_names_its_stable_track_id() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00A1_B2C3);
    let snapshot = TrackSnapshot::new(TrackId::new(9), key, 1_000);

    let handle = TrackSnapshotHandle::new(
        ProducerInstanceId::new(1),
        SnapshotRevision::new(1),
        snapshot,
    );
    assert_eq!(TrackDelta::Created(handle.clone()).id(), TrackId::new(9));
    assert_eq!(TrackDelta::Updated(handle.clone()).id(), TrackId::new(9));
    assert_eq!(TrackDelta::Coasting(handle).id(), TrackId::new(9));
    assert_eq!(
        TrackDelta::Removed {
            producer_instance_id: ProducerInstanceId::new(1),
            snapshot_revision: SnapshotRevision::new(2),
            id: TrackId::new(9),
            key,
            reason: RemovalReason::Expired,
        }
        .id(),
        TrackId::new(9)
    );
}

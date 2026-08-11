#![allow(clippy::expect_used, clippy::panic)]

use surveillance_core::{
    AddressNamespace, Band, DeliveryPath, FieldProvenance, FieldQuality, ObservationOrigin,
    ObservationTime, PositionStatus, ProducerInstanceId, RemovalReason, SnapshotRevision,
    SourceRef, TimedField, TrackDelta, TrackId, TrackKey, TrackSnapshot, TrackSnapshotHandle,
    Wgs84Position,
};

use super::{FeatureDelta, FeatureId, map_track_delta};

const PRODUCER: ProducerInstanceId = ProducerInstanceId::new(17);
const TRACK_ID: TrackId = TrackId::new(41);
const SURVIVOR_ID: TrackId = TrackId::new(42);

fn key(address: u32) -> TrackKey {
    TrackKey::new(AddressNamespace::Icao, address)
}

fn feature_id(track_id: TrackId) -> FeatureId {
    FeatureId::new(PRODUCER, track_id)
}

fn position() -> TimedField<Wgs84Position> {
    TimedField::new(
        Wgs84Position {
            latitude_deg: 37.618_805_6,
            longitude_deg: -122.375_416_7,
        },
        ObservationTime::local(12_000),
        FieldQuality::new(Some(9), Some(10), None, Some(3)),
        FieldProvenance::new(
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            AddressNamespace::Icao,
            SourceRef::new(5, 2, DeliveryPath::LocalReceiver),
        ),
    )
}

fn handle(revision: u64, address: u32, with_position: bool) -> TrackSnapshotHandle {
    handle_for(PRODUCER, revision, address, with_position)
}

fn handle_for(
    producer: ProducerInstanceId,
    revision: u64,
    address: u32,
    with_position: bool,
) -> TrackSnapshotHandle {
    let mut snapshot = TrackSnapshot::new(TRACK_ID, key(address), 10_000);
    snapshot.position = with_position.then(position);
    if with_position {
        snapshot.position_status = PositionStatus::Current;
    }
    snapshot.ownship_shadow = true;
    TrackSnapshotHandle::new(producer, SnapshotRevision::new(revision), snapshot)
}

fn stale_handle(revision: u64, address: u32) -> TrackSnapshotHandle {
    stale_handle_for(PRODUCER, revision, address)
}

fn stale_handle_for(
    producer: ProducerInstanceId,
    revision: u64,
    address: u32,
) -> TrackSnapshotHandle {
    let mut snapshot = TrackSnapshot::new(TRACK_ID, key(address), 10_000);
    snapshot.position = Some(position());
    snapshot.position_status = PositionStatus::Stale;
    TrackSnapshotHandle::new(producer, SnapshotRevision::new(revision), snapshot)
}

#[test]
fn created_and_updated_tracks_with_positions_upsert() {
    for delta in [
        TrackDelta::Created(handle(1, 0x00A1_B2C3, true)),
        TrackDelta::Updated(handle(2, 0x00A1_B2C3, true)),
    ] {
        let Some(FeatureDelta::Upsert(feature)) = map_track_delta(&delta) else {
            panic!("a positioned create or update must upsert");
        };

        assert_eq!(feature.id(), TRACK_ID);
        assert_eq!(feature.feature_id(), feature_id(TRACK_ID));
        assert_eq!(feature.position(), &position());
        assert!(feature.ownship_shadow());
        assert_eq!(feature.producer_instance_id(), PRODUCER);
        assert_eq!(feature.snapshot().position, Some(position()));
        assert_eq!(feature.snapshot_handle().producer_instance_id(), PRODUCER);
    }
}

#[test]
fn stale_position_produces_a_stale_feature_while_the_track_is_active() {
    let delta = TrackDelta::Updated(stale_handle(3, 0x00A1_B2C3));

    assert_eq!(
        map_track_delta(&delta),
        Some(FeatureDelta::Stale {
            id: feature_id(TRACK_ID),
            revision: SnapshotRevision::new(3),
        })
    );
}

#[test]
fn a_current_position_stays_current_when_the_track_coasts() {
    let delta = TrackDelta::Coasting(handle(4, 0x00A1_B2C3, true));

    assert!(matches!(
        map_track_delta(&delta),
        Some(FeatureDelta::Upsert(_))
    ));
}

#[test]
fn feature_events_separate_producer_restarts_and_order_one_feature() {
    let restarted_producer = ProducerInstanceId::new(18);
    let first = map_track_delta(&TrackDelta::Coasting(stale_handle_for(
        PRODUCER,
        7,
        0x00A1_B2C3,
    )))
    .expect("a positioned coasting track produces a stale event");
    let later = map_track_delta(&TrackDelta::Coasting(stale_handle_for(
        PRODUCER,
        8,
        0x00A1_B2C3,
    )))
    .expect("a later coasting state produces a stale event");
    let restarted = map_track_delta(&TrackDelta::Coasting(stale_handle_for(
        restarted_producer,
        1,
        0x00A1_B2C3,
    )))
    .expect("a restarted producer produces a separate stale event");

    let FeatureDelta::Stale {
        id: first_id,
        revision: first_revision,
    } = first
    else {
        panic!("expected a stale event");
    };
    let FeatureDelta::Stale {
        id: later_id,
        revision: later_revision,
    } = later
    else {
        panic!("expected a stale event");
    };
    let FeatureDelta::Stale {
        id: restarted_id, ..
    } = restarted
    else {
        panic!("expected a stale event");
    };

    assert_eq!(first_id, later_id);
    assert!(first_revision < later_revision);
    assert_ne!(first_id, restarted_id);
}

#[test]
fn merge_transfers_and_expiry_removes() {
    let merged = TrackDelta::Removed {
        producer_instance_id: PRODUCER,
        snapshot_revision: SnapshotRevision::new(4),
        id: TRACK_ID,
        key: key(0x00A1_B2C3),
        reason: RemovalReason::Merged { into: SURVIVOR_ID },
    };
    let expired = TrackDelta::Removed {
        producer_instance_id: PRODUCER,
        snapshot_revision: SnapshotRevision::new(5),
        id: TRACK_ID,
        key: key(0x00A1_B2C3),
        reason: RemovalReason::Expired,
    };

    assert_eq!(
        map_track_delta(&merged),
        Some(FeatureDelta::Remove {
            id: feature_id(TRACK_ID),
            revision: SnapshotRevision::new(4),
            transfer_to: Some(feature_id(SURVIVOR_ID)),
        })
    );
    assert_eq!(
        map_track_delta(&expired),
        Some(FeatureDelta::Remove {
            id: feature_id(TRACK_ID),
            revision: SnapshotRevision::new(5),
            transfer_to: None,
        })
    );
}

#[test]
fn position_absent_track_waits_for_its_first_position() {
    let created = TrackDelta::Created(handle(1, 0x00A1_B2C3, false));
    let positioned = TrackDelta::Updated(handle(2, 0x00A1_B2C3, true));
    let expired = TrackDelta::Removed {
        producer_instance_id: PRODUCER,
        snapshot_revision: SnapshotRevision::new(3),
        id: TRACK_ID,
        key: key(0x00A1_B2C3),
        reason: RemovalReason::Expired,
    };

    assert_eq!(map_track_delta(&created), None);
    assert!(matches!(
        map_track_delta(&positioned),
        Some(FeatureDelta::Upsert(_))
    ));
    assert_eq!(
        map_track_delta(&expired),
        Some(FeatureDelta::Remove {
            id: feature_id(TRACK_ID),
            revision: SnapshotRevision::new(3),
            transfer_to: None,
        })
    );
}

#[test]
fn rekeying_keeps_one_feature_identity() {
    let first = TrackDelta::Updated(handle(6, 0x00A1_B2C3, true));
    let rekeyed = TrackDelta::Updated(handle(7, 0x00D4_E5F6, true));

    let Some(FeatureDelta::Upsert(first_feature)) = map_track_delta(&first) else {
        panic!("the first positioned state must upsert");
    };
    let Some(FeatureDelta::Upsert(rekeyed_feature)) = map_track_delta(&rekeyed) else {
        panic!("the re-keyed positioned state must upsert");
    };

    assert_eq!(first_feature.id(), rekeyed_feature.id());
    assert_ne!(first_feature.snapshot().key, rekeyed_feature.snapshot().key);
    assert_eq!(
        rekeyed_feature.snapshot_revision(),
        SnapshotRevision::new(7)
    );
}

#[test]
fn position_absent_coasting_track_has_no_feature_change() {
    let delta = TrackDelta::Coasting(handle(2, 0x00A1_B2C3, false));

    assert_eq!(map_track_delta(&delta), None);
}

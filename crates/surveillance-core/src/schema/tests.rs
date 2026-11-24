#![allow(clippy::expect_used, clippy::panic)]

use crate::schema::{
    CURRENT_TRACK_SCHEMA_VERSION, TrackRecord, TrackRecordHeader, TrackSchemaError,
    validate_supported_version,
};
use crate::track::{
    AddressNamespace, ProducerInstanceId, SnapshotRevision, TrackDelta, TrackId, TrackKey,
    TrackSnapshot, TrackSnapshotHandle,
};

fn created_delta() -> TrackDelta {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AB_1234);
    TrackDelta::Created(TrackSnapshotHandle::new(
        ProducerInstanceId::new(7),
        SnapshotRevision::new(3),
        TrackSnapshot::new(TrackId::new(1), key, 0),
    ))
}

#[test]
fn a_new_record_carries_the_current_version() {
    let record = TrackRecord::new(created_delta());
    assert_eq!(record.schema_version(), CURRENT_TRACK_SCHEMA_VERSION);
    assert!(record.delta().is_ok());
    assert!(record.into_delta().is_ok());
}

#[test]
fn accessors_reject_a_version_this_library_does_not_read() {
    let mut record = TrackRecord::new(created_delta());
    record.schema_version = CURRENT_TRACK_SCHEMA_VERSION + 1;

    let expected = TrackSchemaError::UnsupportedVersion {
        found: CURRENT_TRACK_SCHEMA_VERSION + 1,
        supported: CURRENT_TRACK_SCHEMA_VERSION,
    };
    assert_eq!(record.delta().expect_err("must reject"), expected);
    assert_eq!(record.into_delta().expect_err("must reject"), expected);
}

#[test]
fn a_version_4_reader_rejects_the_version_5_header() {
    let fixture = include_str!("../../tests/fixtures/track_v5_created.json");
    let header: TrackRecordHeader =
        serde_json::from_str(fixture).expect("the version header decodes without the payload");

    assert_eq!(
        validate_supported_version(header.schema_version(), 4).expect_err("version 5 must reject"),
        TrackSchemaError::UnsupportedVersion {
            found: 5,
            supported: 4,
        }
    );
}

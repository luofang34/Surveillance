//! Umbrella re-export contract tests.

#![allow(clippy::expect_used, clippy::panic)]

use surveillance::{
    AddressNamespace, CURRENT_TRACK_SCHEMA_VERSION, TrackDelta, TrackId, TrackKey, TrackRecord,
    TrackSnapshot,
};

#[test]
fn the_umbrella_exports_the_schema_surface() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AB_CDEF);
    let record = TrackRecord::new(TrackDelta::Created(TrackSnapshot::new(
        TrackId::new(1),
        key,
        0,
    )));
    assert_eq!(record.schema_version(), CURRENT_TRACK_SCHEMA_VERSION);

    let serialized = serde_json::to_string(&record).expect("record serializes");
    let decoded: TrackRecord = serde_json::from_str(&serialized).expect("record decodes");
    assert_eq!(decoded, record);
}

#[test]
fn the_module_style_path_stays_available() {
    assert_eq!(surveillance::core::TrackId::new(2).get(), 2);
}

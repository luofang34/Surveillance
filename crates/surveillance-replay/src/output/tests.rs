#![allow(clippy::expect_used, clippy::panic)]

use surveillance_core::{
    AddressNamespace, ProducerInstanceId, RemovalReason, SnapshotRevision, TrackDelta, TrackId,
    TrackKey,
};

use super::*;

const PRODUCER: ProducerInstanceId = ProducerInstanceId::new(71);
const TRACK: TrackId = TrackId::new(9);

fn removed(reason: RemovalReason) -> TrackDelta {
    TrackDelta::Removed {
        producer_instance_id: PRODUCER,
        snapshot_revision: SnapshotRevision::new(4),
        id: TRACK,
        key: TrackKey::new(AddressNamespace::Icao, 0x00A1_B2C3),
        reason,
    }
}

#[test]
fn track_records_are_newline_delimited_versioned_envelopes() {
    let mut bytes = Vec::new();
    let mut writer = DeltaWriter::new(&mut bytes, OutputFormat::TrackRecord)
        .expect("TrackRecord output is always available");
    writer.write_delta(&removed(RemovalReason::Expired));
    writer.finish().expect("memory output succeeds");

    assert_eq!(bytes.last(), Some(&b'\n'));
    let record: TrackRecord = serde_json::from_slice(&bytes).expect("one record decodes");
    assert_eq!(record.delta().expect("the version is current").id(), TRACK);
    assert_eq!(
        record
            .delta()
            .expect("the version is current")
            .producer_instance_id(),
        PRODUCER
    );
}

#[cfg(feature = "geojson")]
#[test]
fn geojson_distinguishes_a_merge_from_an_expiry() {
    let mut bytes = Vec::new();
    let mut writer =
        DeltaWriter::new(&mut bytes, OutputFormat::GeoJson).expect("the feature is enabled");
    writer.write_delta(&removed(RemovalReason::Merged {
        into: TrackId::new(10),
    }));
    writer.write_delta(&removed(RemovalReason::Expired));
    writer.finish().expect("memory output succeeds");

    let values: Vec<serde_json::Value> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("each line is JSON"))
        .collect();
    assert_eq!(values[0]["properties"]["transfer_to"], "71:10");
    assert_eq!(
        values[1]["properties"]["transfer_to"],
        serde_json::Value::Null
    );
}

#[cfg(not(feature = "geojson"))]
#[test]
fn geojson_names_the_feature_that_enables_it() {
    assert!(matches!(
        DeltaWriter::new(Vec::new(), OutputFormat::GeoJson),
        Err(OutputError::GeoJsonFeatureDisabled)
    ));
}

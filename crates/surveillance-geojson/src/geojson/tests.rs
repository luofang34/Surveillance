#![allow(clippy::expect_used, clippy::panic)]

use serde_json::{Value, json};
use surveillance_core::{
    AddressNamespace, Band, DeliveryPath, FieldProvenance, FieldQuality, ObservationOrigin,
    ObservationTime, ProducerInstanceId, SnapshotRevision, SourceRef, TimedField, TrackDelta,
    TrackId, TrackKey, TrackSnapshot, TrackSnapshotHandle, Wgs84Position,
};

use crate::{FeatureDelta, map_track_delta};

use super::{GeoJsonError, encode_feature_delta};

const ID: TrackId = TrackId::new(91);

fn positioned_feature(latitude_deg: f64, longitude_deg: f64) -> FeatureDelta {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00A1_B2C3);
    let mut snapshot = TrackSnapshot::new(ID, key, 700);
    snapshot.ownship_shadow = true;
    snapshot.position = Some(TimedField::new(
        Wgs84Position {
            latitude_deg,
            longitude_deg,
        },
        ObservationTime::local(800),
        FieldQuality::new(Some(8), Some(9), None, Some(2)),
        FieldProvenance::new(
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            AddressNamespace::Icao,
            SourceRef::new(3, 4, DeliveryPath::LocalReceiver),
        ),
    ));
    let delta = TrackDelta::Updated(TrackSnapshotHandle::new(
        ProducerInstanceId::new(12),
        SnapshotRevision::new(6),
        snapshot,
    ));

    map_track_delta(&delta).expect("a positioned update must map")
}

fn parse(delta: &FeatureDelta) -> Value {
    let encoded = encode_feature_delta(delta).expect("valid feature must encode");
    serde_json::from_str(&encoded).expect("encoded feature must be JSON")
}

#[test]
fn upsert_is_an_rfc_7946_point_with_exported_properties() {
    let value = parse(&positioned_feature(37.618_805_6, -122.375_416_7));

    assert_eq!(value["type"], "Feature");
    assert_eq!(value["id"], 91);
    assert_eq!(value["geometry"]["type"], "Point");
    assert_eq!(
        value["geometry"]["coordinates"],
        json!([-122.375_416_7, 37.618_805_6])
    );
    assert_eq!(value["properties"]["operation"], "upsert");
    assert_eq!(value["properties"]["producer_instance_id"], 12);
    assert_eq!(value["properties"]["snapshot_revision"], 6);
    assert_eq!(value["properties"]["ownship_shadow"], true);
    assert_eq!(
        value["properties"]["position"]["provenance"]["source"]["id"],
        3
    );
    assert_eq!(
        value["properties"]["position"]["provenance"]["source"]["epoch"],
        4
    );
}

#[test]
fn stale_and_remove_are_features_with_null_geometry() {
    let stale = parse(&FeatureDelta::Stale { id: ID });
    let merged = parse(&FeatureDelta::Remove {
        id: ID,
        transfer_to: Some(TrackId::new(92)),
    });
    let expired = parse(&FeatureDelta::Remove {
        id: ID,
        transfer_to: None,
    });

    assert_eq!(stale["geometry"], Value::Null);
    assert_eq!(stale["properties"]["operation"], "stale");
    assert_eq!(merged["geometry"], Value::Null);
    assert_eq!(merged["properties"]["operation"], "remove");
    assert_eq!(merged["properties"]["transfer_to"], 92);
    assert_eq!(expired["properties"]["transfer_to"], Value::Null);
}

#[test]
fn invalid_wgs84_coordinates_return_a_typed_error() {
    let delta = positioned_feature(91.0, -122.0);

    assert!(matches!(
        encode_feature_delta(&delta),
        Err(GeoJsonError::InvalidPosition {
            track_id: ID,
            latitude_deg: 91.0,
            longitude_deg: -122.0,
        })
    ));
}

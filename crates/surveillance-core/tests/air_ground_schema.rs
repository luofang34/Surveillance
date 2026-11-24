//! Wire form and version of the generic airborne air/ground state.

#![allow(clippy::expect_used, clippy::panic)]

use surveillance_core::{
    AirGroundState, CURRENT_TRACK_SCHEMA_VERSION, TrackDelta, TrackRecord, TrackRecordHeader,
    TrackSchemaError,
};

const CREATED_FIXTURE: &str = include_str!("fixtures/track_v5_created.json");

/// A version 4 reader has no `airborne` value for the air/ground state. A
/// record that can contain the value must thus declare a version that such a
/// reader refuses.
#[test]
fn the_schema_before_the_generic_airborne_state_is_refused_by_version() {
    let prior = CREATED_FIXTURE.replacen("\"schema_version\": 5", "\"schema_version\": 4", 1);
    let header: TrackRecordHeader =
        serde_json::from_str(&prior).expect("the version header decodes without the payload");

    assert_eq!(
        header.validate().expect_err("the prior schema must reject"),
        TrackSchemaError::UnsupportedVersion {
            found: 4,
            supported: CURRENT_TRACK_SCHEMA_VERSION,
        }
    );
}

#[test]
fn the_generic_airborne_state_has_its_own_wire_value() {
    let value: serde_json::Value =
        serde_json::from_str(CREATED_FIXTURE).expect("fixture JSON parses");
    assert_eq!(
        value["delta"]["data"]["snapshot"]["air_ground"]["value"],
        "airborne"
    );

    let decoded: TrackRecord = serde_json::from_str(CREATED_FIXTURE).expect("fixture decodes");
    let TrackDelta::Created(snapshot) = decoded.delta().expect("current version") else {
        panic!("fixture is a created delta");
    };
    let state = snapshot.air_ground.as_ref().expect("fixture has a state");
    assert_eq!(state.value, AirGroundState::Airborne);
    assert_eq!(state.value.is_airborne(), Some(true));
}

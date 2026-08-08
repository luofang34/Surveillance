//! Public exported-schema contract tests: golden fixture, bit-exact floats,
//! absent-stays-absent, and version validation before payload interpretation.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::field_reassign_with_default
)]

use surveillance_core::{
    AddressNamespace, AirGroundState, Band, CURRENT_TRACK_SCHEMA_VERSION, Callsign, CallsignKind,
    DeliveryPath, EmergencyState, FieldProvenance, FieldQuality, ObservationOrigin,
    ObservationTime, SourceRef, Squawk, TimedField, TrackDelta, TrackId, TrackKey, TrackRecord,
    TrackRecordHeader, TrackSchemaError, TrackSnapshot, VelocityObservation, VerticalRateSource,
    Wgs84Position,
};

const CREATED_FIXTURE: &str = include_str!("fixtures/track_v2_created.json");
/// A record written before origin, delivery, and time were separated.
const SUPERSEDED_FIXTURE: &str = include_str!("fixtures/track_v1_created_superseded.json");

const KEY: TrackKey = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
const TRACK_FILE_KEY: TrackKey = TrackKey::new(AddressNamespace::TisbTrackFile, 66);

fn adsb_provenance() -> FieldProvenance {
    FieldProvenance::new(
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
        AddressNamespace::Icao,
        SourceRef::new(1, 0, DeliveryPath::LocalReceiver),
    )
}

fn fixture_record() -> TrackRecord {
    let mut snapshot = TrackSnapshot::new(TrackId::new(42), KEY, 1_000_000);
    snapshot.identities.associate(TRACK_FILE_KEY);
    snapshot.position = Some(TimedField::new(
        Wgs84Position {
            latitude_deg: 40.639751,
            longitude_deg: -73.778925,
        },
        ObservationTime::local(1_500_000),
        FieldQuality::new(Some(8), Some(9), None, Some(2)),
        adsb_provenance(),
    ));
    snapshot.pressure_altitude_ft = Some(TimedField::new(
        12_500,
        ObservationTime::local(1_500_000),
        FieldQuality::default(),
        adsb_provenance(),
    ));
    let mut velocity = VelocityObservation::default();
    velocity.ground_speed_kt = Some(214.5);
    velocity.track_angle_deg_true = Some(87.25);
    velocity.vertical_rate_fpm = Some(-640);
    velocity.vertical_rate_source = Some(VerticalRateSource::Barometric);
    snapshot.velocity = Some(TimedField::new(
        velocity,
        ObservationTime::local(1_500_000),
        FieldQuality::new(None, None, Some(2), None),
        adsb_provenance(),
    ));
    let mut callsign = Callsign::new("DAL123");
    callsign.kind = Some(CallsignKind::Callsign);
    snapshot.callsign = Some(TimedField::new(
        callsign,
        ObservationTime::local(1_200_000),
        FieldQuality::default(),
        adsb_provenance(),
    ));
    snapshot.squawk = Some(TimedField::new(
        Squawk::try_from(3471).expect("valid Mode A code"),
        ObservationTime::local(1_200_000),
        FieldQuality::default(),
        adsb_provenance(),
    ));
    snapshot.air_ground = Some(TimedField::new(
        AirGroundState::Subsonic,
        ObservationTime::local(1_500_000),
        FieldQuality::default(),
        adsb_provenance(),
    ));
    snapshot.emergency = Some(TimedField::new(
        EmergencyState::None,
        ObservationTime::local(1_200_000),
        FieldQuality::default(),
        adsb_provenance(),
    ));
    snapshot.last_observed_at_micros = 1_500_000;
    TrackRecord::new(TrackDelta::Created(snapshot))
}

#[test]
fn the_golden_fixture_decodes_to_the_constructed_record() {
    let decoded: TrackRecord = serde_json::from_str(CREATED_FIXTURE).expect("fixture decodes");
    assert_eq!(decoded, fixture_record());
}

#[test]
fn serialization_round_trips_the_golden_fixture() {
    let record = fixture_record();
    let serialized = serde_json::to_string(&record).expect("record serializes");

    let reparsed: TrackRecord = serde_json::from_str(&serialized).expect("round trip decodes");
    assert_eq!(reparsed, record);

    let serialized_value: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON parses");
    let fixture_value: serde_json::Value =
        serde_json::from_str(CREATED_FIXTURE).expect("fixture JSON parses");
    assert_eq!(serialized_value, fixture_value);
}

#[test]
fn float_fields_round_trip_bit_exact() {
    let record = fixture_record();
    let serialized = serde_json::to_string(&record).expect("record serializes");
    let reparsed: TrackRecord = serde_json::from_str(&serialized).expect("round trip decodes");

    let delta = reparsed.delta().expect("current version");
    let TrackDelta::Created(snapshot) = delta else {
        panic!("fixture is a created delta");
    };
    let position = snapshot.position.as_ref().expect("fixture has a position");
    assert_eq!(
        position.value.latitude_deg.to_bits(),
        40.639751_f64.to_bits()
    );
    assert_eq!(
        position.value.longitude_deg.to_bits(),
        (-73.778925_f64).to_bits()
    );
}

#[test]
fn absent_fields_stay_absent_on_the_wire() {
    let record = TrackRecord::new(TrackDelta::Created(TrackSnapshot::new(
        TrackId::new(7),
        KEY,
        5_000,
    )));
    let serialized = serde_json::to_string(&record).expect("record serializes");

    let value: serde_json::Value = serde_json::from_str(&serialized).expect("JSON parses");
    let data = &value["delta"]["data"];
    let object = data.as_object().expect("snapshot is an object");
    for absent in [
        "position",
        "pressure_altitude_ft",
        "geometric_altitude_ft",
        "velocity",
        "callsign",
        "squawk",
        "air_ground",
        "emergency",
    ] {
        assert!(!object.contains_key(absent), "{absent} must be absent");
    }

    let reparsed: TrackRecord = serde_json::from_str(&serialized).expect("round trip decodes");
    assert_eq!(reparsed, record);
}

#[test]
fn unsupported_versions_fail_with_a_typed_error() {
    let unsupported =
        CREATED_FIXTURE.replacen("\"schema_version\": 2", "\"schema_version\": 99", 1);
    let record: TrackRecord =
        serde_json::from_str(&unsupported).expect("payload shape still decodes");

    let expected = TrackSchemaError::UnsupportedVersion {
        found: 99,
        supported: CURRENT_TRACK_SCHEMA_VERSION,
    };
    assert_eq!(record.delta().expect_err("must reject"), expected);
    assert_eq!(record.into_delta().expect_err("must reject"), expected);
}

#[test]
fn the_header_validates_before_any_payload_interpretation() {
    let current: TrackRecordHeader =
        serde_json::from_str(CREATED_FIXTURE).expect("header decodes from a full record");
    assert_eq!(current.schema_version(), CURRENT_TRACK_SCHEMA_VERSION);
    assert!(current.validate().is_ok());

    let foreign = r#"{ "schema_version": 99, "delta": { "kind": "renamed_kind", "data": { "unknown": true } } }"#;
    let header: TrackRecordHeader =
        serde_json::from_str(foreign).expect("header decodes without understanding the payload");
    assert_eq!(
        header.validate().expect_err("must reject"),
        TrackSchemaError::UnsupportedVersion {
            found: 99,
            supported: CURRENT_TRACK_SCHEMA_VERSION,
        }
    );
}

/// A record written against the schema that predates the origin, delivery,
/// and time split must be refused by version rather than misread. Its
/// `link` and bare `observed_at_micros` would otherwise be silently dropped
/// by field-absent defaulting, leaving a track whose provenance claims less
/// than the record actually said.
#[test]
fn a_superseded_record_is_refused_by_version_not_misparsed() {
    let header: TrackRecordHeader =
        serde_json::from_str(SUPERSEDED_FIXTURE).expect("the header decodes from any version");
    assert_eq!(header.schema_version(), 1);
    assert_eq!(
        header.validate().expect_err("version 1 is no longer read"),
        TrackSchemaError::UnsupportedVersion {
            found: 1,
            supported: CURRENT_TRACK_SCHEMA_VERSION,
        }
    );
}

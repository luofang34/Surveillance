//! Public exported-schema contract tests: golden fixture, bit-exact floats,
//! absent-stays-absent, and version validation before payload interpretation.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::field_reassign_with_default
)]

use core::cell::Cell;

use surveillance_core::{
    AddressNamespace, AirGroundState, Band, CURRENT_TRACK_SCHEMA_VERSION, Callsign, CallsignKind,
    DeliveryPath, EmergencyState, FieldProvenance, FieldQuality, FreshnessPolicy,
    ObservationOrigin, ObservationTime, PositionStatus, ProducerInstanceId, ProjectionBounds,
    RemovalReason, SnapshotRevision, SourceRef, Squawk, TimeBasis, TimeCorrespondence, TimedField,
    TrackDelta, TrackId, TrackKey, TrackPhase, TrackRecord, TrackRecordDecodeError,
    TrackRecordHeader, TrackSchemaError, TrackSnapshot, TrackSnapshotHandle, TrackStreamHeader,
    VelocityObservation, VerticalRateSource, Wgs84Position, decode_track_record_with,
};

const CREATED_FIXTURE: &str = include_str!("fixtures/track_v5_created.json");
/// A record written before origin, delivery, and time were separated.
const SUPERSEDED_FIXTURE: &str = include_str!("fixtures/track_v1_created_superseded.json");
/// A record written before snapshots carried publication metadata.
const VERSION_2_FIXTURE: &str = include_str!("fixtures/track_v2_created.json");
/// A non-radio record: the wire forms the ADS-B fixture never reaches.
const PROVIDER_FIXTURE: &str = include_str!("fixtures/track_v5_provider.json");

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
    snapshot.position_status = PositionStatus::Current;
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
        AirGroundState::Airborne,
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
    snapshot.projection_bounds =
        Some(ProjectionBounds::from_freshness(&FreshnessPolicy::default()));
    TrackRecord::new(TrackDelta::Created(TrackSnapshotHandle::new(
        ProducerInstanceId::new(11),
        SnapshotRevision::new(29),
        snapshot,
    )))
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
    let record = TrackRecord::new(TrackDelta::Created(TrackSnapshotHandle::new(
        ProducerInstanceId::new(11),
        SnapshotRevision::new(30),
        TrackSnapshot::new(TrackId::new(7), KEY, 5_000),
    )));
    let serialized = serde_json::to_string(&record).expect("record serializes");

    let value: serde_json::Value = serde_json::from_str(&serialized).expect("JSON parses");
    let data = &value["delta"]["data"]["snapshot"];
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
        CREATED_FIXTURE.replacen("\"schema_version\": 5", "\"schema_version\": 99", 1);
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

#[test]
fn a_version_2_record_is_refused_before_its_snapshot_is_interpreted() {
    let header: TrackRecordHeader =
        serde_json::from_str(VERSION_2_FIXTURE).expect("the header decodes from any version");
    assert_eq!(header.schema_version(), 2);
    assert_eq!(
        header.validate().expect_err("version 2 is no longer read"),
        TrackSchemaError::UnsupportedVersion {
            found: 2,
            supported: CURRENT_TRACK_SCHEMA_VERSION,
        }
    );
}

#[test]
fn the_schema_before_position_status_is_refused_by_version() {
    let prior = CREATED_FIXTURE.replacen("\"schema_version\": 5", "\"schema_version\": 3", 1);
    let header: TrackRecordHeader =
        serde_json::from_str(&prior).expect("the version header decodes without the payload");

    assert_eq!(
        header.validate().expect_err("the prior schema must reject"),
        TrackSchemaError::UnsupportedVersion {
            found: 3,
            supported: CURRENT_TRACK_SCHEMA_VERSION,
        }
    );
}

#[test]
fn a_removal_keeps_its_producer_and_revision_on_the_wire() {
    let record = TrackRecord::new(TrackDelta::Removed {
        producer_instance_id: ProducerInstanceId::new(11),
        snapshot_revision: SnapshotRevision::new(31),
        id: TrackId::new(42),
        key: KEY,
        reason: RemovalReason::Expired,
    });

    let serialized = serde_json::to_string(&record).expect("record serializes");
    let value: serde_json::Value = serde_json::from_str(&serialized).expect("JSON parses");
    let data = &value["delta"]["data"];
    assert_eq!(data["producer_instance_id"], 11);
    assert_eq!(data["snapshot_revision"], 31);

    let decoded: TrackRecord = serde_json::from_str(&serialized).expect("record decodes");
    assert_eq!(decoded, record);
}

/// A provider-fused record with a mapped time, an unknown-time field, and a
/// non-radio origin. The ADS-B fixture exercises none of those wire forms,
/// so a change to any of them would otherwise round-trip untested.
fn provider_record() -> TrackRecord {
    let key = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);
    let provider = SourceRef::new(7, 2, DeliveryPath::NetworkProvider);
    let provenance = FieldProvenance::new(
        ObservationOrigin::ProviderFused,
        AddressNamespace::Icao,
        provider,
    );

    let mut snapshot = TrackSnapshot::new(TrackId::new(9), key, 2_000_000);
    snapshot.phase = TrackPhase::Coasting;
    snapshot.position = Some(TimedField::new(
        Wgs84Position {
            latitude_deg: -33.946111,
            longitude_deg: 151.177222,
        },
        ObservationTime::mapped(2_500_000, 2_100_000, 75_000),
        FieldQuality::new(Some(7), Some(8), None, Some(1)),
        provenance,
    ));
    snapshot.position_status = PositionStatus::Stale;
    // A second source that would not say when it measured.
    snapshot.pressure_altitude_ft = Some(TimedField::new(
        31_000,
        ObservationTime::unknown_source_time(2_500_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::Unknown,
            AddressNamespace::Icao,
            SourceRef::new(8, 0, DeliveryPath::Unspecified),
        ),
    ));
    snapshot.last_observed_at_micros = 2_500_000;
    TrackRecord::new(TrackDelta::Updated(TrackSnapshotHandle::new(
        ProducerInstanceId::new(12),
        SnapshotRevision::new(8),
        snapshot,
    )))
}

#[test]
fn the_non_radio_fixture_round_trips_with_its_time_bases_intact() {
    let decoded: TrackRecord =
        serde_json::from_str(PROVIDER_FIXTURE).expect("provider fixture decodes");
    assert_eq!(decoded, provider_record());

    let serialized = serde_json::to_string(&provider_record()).expect("record serializes");
    let fixture_value: serde_json::Value =
        serde_json::from_str(PROVIDER_FIXTURE).expect("fixture JSON parses");
    let serialized_value: serde_json::Value =
        serde_json::from_str(&serialized).expect("serialized JSON parses");
    assert_eq!(serialized_value, fixture_value);
}

#[test]
fn a_mapped_time_keeps_its_uncertainty_through_a_round_trip() {
    let decoded: TrackRecord =
        serde_json::from_str(PROVIDER_FIXTURE).expect("provider fixture decodes");
    let TrackDelta::Updated(snapshot) = decoded.into_delta().expect("current version") else {
        panic!("the provider fixture is an update");
    };

    let position = snapshot.position.as_ref().expect("position present");
    assert_eq!(
        position.time.basis,
        TimeBasis::Mapped {
            uncertainty_micros: 75_000
        },
        "uncertainty is what makes a mapped time honest; it must survive the wire"
    );

    let altitude = snapshot
        .pressure_altitude_ft
        .as_ref()
        .expect("altitude present");
    assert_eq!(altitude.time.basis, TimeBasis::UnknownSourceTime);
    assert!(
        !altitude.time.is_established(),
        "an unknown source time must not decode as an established one"
    );
}

#[test]
fn the_two_phase_decoder_never_reads_an_unsupported_payload() {
    let encoded = br#"{"schema_version":99,"delta":{"kind":"unknown"}}"#;
    let payload_was_read = Cell::new(false);

    let result = decode_track_record_with(
        encoded,
        serde_json::from_slice,
        |bytes| -> Result<TrackRecord, serde_json::Error> {
            payload_was_read.set(true);
            serde_json::from_slice(bytes)
        },
    );

    assert!(!payload_was_read.get());
    assert!(matches!(
        result,
        Err(TrackRecordDecodeError::Schema(
            TrackSchemaError::UnsupportedVersion {
                found: 99,
                supported: CURRENT_TRACK_SCHEMA_VERSION,
            }
        ))
    ));
}

#[test]
fn an_absent_version_is_rejected_before_payload_decode() {
    let encoded = br#"{"delta":{"kind":"unknown"}}"#;
    let payload_was_read = Cell::new(false);

    let result = decode_track_record_with(
        encoded,
        serde_json::from_slice,
        |bytes| -> Result<TrackRecord, serde_json::Error> {
            payload_was_read.set(true);
            serde_json::from_slice(bytes)
        },
    );

    assert!(!payload_was_read.get());
    assert!(matches!(
        result,
        Err(TrackRecordDecodeError::Schema(
            TrackSchemaError::UnsupportedVersion {
                found: 0,
                supported: CURRENT_TRACK_SCHEMA_VERSION,
            }
        ))
    ));
}

#[test]
fn an_unknown_optional_field_is_a_compatible_addition() {
    let mut value: serde_json::Value =
        serde_json::from_str(CREATED_FIXTURE).expect("fixture JSON parses");
    value.as_object_mut().expect("record is an object").insert(
        "future_optional".to_owned(),
        serde_json::json!({"value": 7}),
    );
    let encoded = serde_json::to_vec(&value).expect("extended record serializes");

    let decoded =
        decode_track_record_with(&encoded, serde_json::from_slice, serde_json::from_slice)
            .expect("an older reader ignores an optional addition");
    assert_eq!(decoded, fixture_record());
}

#[test]
fn a_required_field_rename_is_an_incompatible_payload_change() {
    let mut value: serde_json::Value =
        serde_json::from_str(CREATED_FIXTURE).expect("fixture JSON parses");
    let object = value.as_object_mut().expect("record is an object");
    let delta = object.remove("delta").expect("fixture has a delta");
    object.insert("renamed_delta".to_owned(), delta);
    let encoded = serde_json::to_vec(&value).expect("changed record serializes");

    let result = decode_track_record_with(&encoded, serde_json::from_slice, serde_json::from_slice);
    assert!(matches!(result, Err(TrackRecordDecodeError::Record { .. })));
}

#[test]
fn one_stream_correspondence_places_record_times_in_utc() {
    let correspondence = TimeCorrespondence {
        monotonic_micros: 2_000_000,
        utc_unix_micros: 1_725_000_000_000_000,
    };
    let header = TrackStreamHeader::new(correspondence);
    let record = provider_record();
    let serialized_header = serde_json::to_value(header).expect("header serializes");
    let serialized_record = serde_json::to_value(&record).expect("record serializes");

    assert_eq!(
        serialized_header["time_correspondence"]["monotonic_micros"],
        2_000_000
    );
    assert!(serialized_record.get("time_correspondence").is_none());

    let TrackDelta::Updated(snapshot) = record.into_delta().expect("current version") else {
        panic!("the provider fixture is an update");
    };
    let utc = header
        .time_correspondence()
        .expect("current stream version")
        .utc_unix_micros_at(snapshot.last_observed_at_micros)
        .expect("UTC result is in range");
    assert_eq!(utc, 1_725_000_000_500_000);
}

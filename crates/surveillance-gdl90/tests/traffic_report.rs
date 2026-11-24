//! Public GDL 90 Traffic Report encoding tests.

#![allow(clippy::expect_used, clippy::panic)]

use surveillance_core::{
    AddressNamespace, AirGroundState, Callsign, EmergencyState, EmitterCategory, FieldProvenance,
    FieldQuality, HeadingReference, ObservationOrigin, ObservationTime, PositionStatus, SourceRef,
    TimedField, TrackId, TrackKey, TrackPhase, TrackSnapshot, VelocityObservation, Wgs84Position,
};
use surveillance_gdl90::{
    GDL90_TRAFFIC_REPORT_LEN, TrafficAlertStatus, TrafficReportError, encode_traffic_report,
};

fn provenance(namespace: AddressNamespace) -> FieldProvenance {
    FieldProvenance::new(
        ObservationOrigin::AdsbDirect {
            band: surveillance_core::Band::Adsb1090,
        },
        namespace,
        SourceRef::default(),
    )
}

fn timed<T>(value: T, namespace: AddressNamespace) -> TimedField<T> {
    TimedField::new(
        value,
        ObservationTime::local(1_000),
        FieldQuality::default(),
        provenance(namespace),
    )
}

fn required_snapshot() -> TrackSnapshot {
    let namespace = AddressNamespace::Icao;
    let mut snapshot = TrackSnapshot::new(
        TrackId::new(1),
        TrackKey::new(namespace, 0x00ab_4549),
        1_000,
    );
    snapshot.air_ground = Some(timed(AirGroundState::Ground, namespace));
    snapshot.emergency = Some(timed(EmergencyState::None, namespace));
    snapshot
}

fn example_velocity() -> VelocityObservation {
    let mut velocity = VelocityObservation::default();
    velocity.ground_speed_kt = Some(123.0);
    velocity.track_angle_deg_true = Some(45.0);
    velocity.vertical_rate_fpm = Some(64);
    velocity
}

fn track_velocity(angle_deg: f64) -> VelocityObservation {
    let mut velocity = VelocityObservation::default();
    velocity.track_angle_deg_true = Some(angle_deg);
    velocity
}

fn heading_velocity(angle_deg: f64, reference: HeadingReference) -> VelocityObservation {
    let mut velocity = VelocityObservation::default();
    velocity.heading_deg = Some(angle_deg);
    velocity.heading_reference = Some(reference);
    velocity
}

#[test]
fn encodes_the_public_icd_traffic_report_example() {
    let namespace = AddressNamespace::Icao;
    let mut snapshot = required_snapshot();
    let latitude_code = 0x001f_ef15_i32;
    let longitude_code = i32::from_be_bytes([0xff, 0xa8, 0x89, 0x78]);
    snapshot.position = Some(TimedField::new(
        Wgs84Position {
            latitude_deg: f64::from(latitude_code) * 180.0 / 8_388_608.0,
            longitude_deg: f64::from(longitude_code) * 180.0 / 8_388_608.0,
        },
        ObservationTime::local(1_000),
        FieldQuality::new(Some(10), Some(9), None, None),
        provenance(namespace),
    ));
    snapshot.position_status = PositionStatus::Current;
    snapshot.pressure_altitude_ft = Some(timed(5_000, namespace));
    snapshot.velocity = Some(timed(example_velocity(), namespace));
    snapshot.callsign = Some(timed(Callsign::new("N825V"), namespace));
    snapshot.air_ground = Some(timed(AirGroundState::Subsonic, namespace));
    snapshot.emitter_category = Some(timed(EmitterCategory::LightAircraft, namespace));

    let mut output = [0_u8; GDL90_TRAFFIC_REPORT_LEN];
    let encoded = encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect("the ICD example is representable");

    assert_eq!(encoded.bytes_written, GDL90_TRAFFIC_REPORT_LEN);
    assert!(!encoded.ownship_shadow);
    assert_eq!(
        output,
        [
            0x14, 0x00, 0xab, 0x45, 0x49, 0x1f, 0xef, 0x15, 0xa8, 0x89, 0x78, 0x0f, 0x09, 0xa9,
            0x07, 0xb0, 0x01, 0x20, 0x01, 0x4e, 0x38, 0x32, 0x35, 0x56, 0x20, 0x20, 0x20, 0x00,
        ]
    );
}

#[test]
fn uses_specification_codes_for_absent_optional_values() {
    let snapshot = required_snapshot();
    let mut output = [0xaa_u8; GDL90_TRAFFIC_REPORT_LEN];

    encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect("required state is present");

    assert_eq!(
        output,
        [
            0x14, 0x00, 0xab, 0x45, 0x49, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf0, 0x00,
            0xff, 0xf8, 0x00, 0x00, 0x00, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00,
        ]
    );
    assert_eq!(&output[5..11], &[0; 6], "position and NIC are unavailable");
    assert_eq!(
        u16::from(output[11]) << 4 | u16::from(output[12] >> 4),
        0x0fff
    );
    assert_eq!(output[13], 0, "NIC and NACp are unknown");
    assert_eq!(
        u16::from(output[14]) << 4 | u16::from(output[15] >> 4),
        0x0fff
    );
    assert_eq!(
        u16::from(output[15] & 0x0f) << 8 | u16::from(output[16]),
        0x0800
    );
    assert_eq!(output[12] & 0x03, 0, "track or heading is not valid");
    assert_eq!(output[18], 0, "emitter category is unknown");
    assert_eq!(&output[19..27], b"        ", "callsign is unavailable");
}

#[test]
fn generic_airborne_state_sets_the_airborne_bit() {
    let namespace = AddressNamespace::Icao;
    let mut snapshot = required_snapshot();
    snapshot.air_ground = Some(timed(AirGroundState::Airborne, namespace));
    let mut output = [0_u8; GDL90_TRAFFIC_REPORT_LEN];

    encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect("generic airborne state is representable");

    assert_eq!(output[12] & 0x08, 0x08);
}

#[test]
fn returns_policy_metadata_without_suppressing_the_report() {
    let mut snapshot = required_snapshot();
    snapshot.key.namespace = AddressNamespace::SurfaceVehicle;
    snapshot.phase = TrackPhase::Coasting;
    snapshot.ownship_shadow = true;
    snapshot.air_ground = None;
    let mut output = [0_u8; GDL90_TRAFFIC_REPORT_LEN];

    let encoded = encode_traffic_report(&snapshot, TrafficAlertStatus::TrafficAlert, &mut output)
        .expect("a surface vehicle has an implicit ground state");

    assert!(encoded.ownship_shadow);
    assert_eq!(
        output,
        [
            0x14, 0x14, 0xab, 0x45, 0x49, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf4, 0x00,
            0xff, 0xf8, 0x00, 0x00, 0x00, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00,
        ]
    );
}

#[test]
fn leaves_a_short_buffer_unchanged() {
    let snapshot = required_snapshot();
    let mut output = [0xa5_u8; GDL90_TRAFFIC_REPORT_LEN - 1];

    let error = encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect_err("the buffer is one byte short");

    assert_eq!(
        error,
        TrafficReportError::BufferCapacity {
            required: GDL90_TRAFFIC_REPORT_LEN,
            available: GDL90_TRAFFIC_REPORT_LEN - 1,
        }
    );
    assert_eq!(output, [0xa5; GDL90_TRAFFIC_REPORT_LEN - 1]);
}

#[test]
fn rejects_missing_required_state_without_changing_output() {
    let mut snapshot = required_snapshot();
    snapshot.air_ground = None;
    let mut output = [0xa5_u8; GDL90_TRAFFIC_REPORT_LEN];

    let error = encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect_err("air or ground state has no unavailable code");
    assert_eq!(error, TrafficReportError::AirGroundUnavailable);
    assert_eq!(output, [0xa5; GDL90_TRAFFIC_REPORT_LEN]);

    snapshot.air_ground = Some(timed(AirGroundState::Ground, AddressNamespace::Icao));
    snapshot.emergency = None;
    let error = encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect_err("emergency state has no unavailable code");
    assert_eq!(error, TrafficReportError::EmergencyUnavailable);
    assert_eq!(output, [0xa5; GDL90_TRAFFIC_REPORT_LEN]);
}

#[test]
fn rejects_a_position_state_without_a_position() {
    let mut snapshot = required_snapshot();
    snapshot.position_status = PositionStatus::Current;
    let mut output = [0xa5_u8; GDL90_TRAFFIC_REPORT_LEN];

    let error = encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect_err("current position state requires a value");

    assert_eq!(error, TrafficReportError::PositionStateMismatch);
    assert_eq!(output, [0xa5; GDL90_TRAFFIC_REPORT_LEN]);
}

#[test]
fn distinguishes_adsb_and_tisb_icao_addresses_from_provenance() {
    let namespace = AddressNamespace::Icao;
    let mut snapshot = required_snapshot();
    snapshot.air_ground = Some(TimedField::new(
        AirGroundState::Ground,
        ObservationTime::local(1_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::Tisb {
                band: surveillance_core::Band::Uat978,
            },
            namespace,
            SourceRef::default(),
        ),
    ));
    snapshot.emergency = Some(TimedField::new(
        EmergencyState::None,
        ObservationTime::local(1_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::Tisb {
                band: surveillance_core::Band::Uat978,
            },
            namespace,
            SourceRef::default(),
        ),
    ));
    let mut output = [0_u8; GDL90_TRAFFIC_REPORT_LEN];

    encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect("TIS-B supplies a GDL 90 ICAO address type");

    assert_eq!(output[1], 2);
}

#[test]
fn distinguishes_track_and_heading_references() {
    let namespace = AddressNamespace::Icao;
    let cases = [
        (track_velocity(90.0), 1),
        (heading_velocity(90.0, HeadingReference::MagneticNorth), 2),
        (heading_velocity(90.0, HeadingReference::TrueNorth), 3),
    ];

    for (velocity, direction_kind) in cases {
        let mut snapshot = required_snapshot();
        snapshot.velocity = Some(timed(velocity, namespace));
        let mut output = [0_u8; GDL90_TRAFFIC_REPORT_LEN];
        encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
            .expect("the direction is representable");
        assert_eq!(output[12] & 0x03, direction_kind);
        assert_eq!(output[17], 64);
    }
}

#[test]
fn refuses_an_unclassified_non_icao_address() {
    let mut snapshot = required_snapshot();
    snapshot.key.namespace = AddressNamespace::AdsbNonIcao;
    let mut output = [0xa5_u8; GDL90_TRAFFIC_REPORT_LEN];

    let error = encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect_err("an unclassified address can denote several GDL 90 types");

    assert_eq!(
        error,
        TrafficReportError::AddressTypeUnavailable { key: snapshot.key }
    );
    assert_eq!(output, [0xa5; GDL90_TRAFFIC_REPORT_LEN]);
}

#[test]
fn refuses_to_label_a_mode_s_only_target_as_adsb() {
    let namespace = AddressNamespace::Icao;
    let mode_s = FieldProvenance::new(
        ObservationOrigin::ModeSReply,
        namespace,
        SourceRef::default(),
    );
    let mut snapshot = required_snapshot();
    snapshot.air_ground = Some(TimedField::new(
        AirGroundState::Subsonic,
        ObservationTime::local(1_000),
        FieldQuality::default(),
        mode_s,
    ));
    snapshot.emergency = Some(TimedField::new(
        EmergencyState::None,
        ObservationTime::local(1_000),
        FieldQuality::default(),
        mode_s,
    ));
    let mut output = [0xa5_u8; GDL90_TRAFFIC_REPORT_LEN];

    let error = encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect_err("GDL 90 defines no Mode S-only address type");

    assert_eq!(
        error,
        TrafficReportError::AddressTypeUnavailable { key: snapshot.key }
    );
    assert_eq!(output, [0xa5; GDL90_TRAFFIC_REPORT_LEN]);
}

#[test]
fn refuses_unused_position_quality_codes() {
    let namespace = AddressNamespace::Icao;
    let mut snapshot = required_snapshot();
    snapshot.position = Some(TimedField::new(
        Wgs84Position {
            latitude_deg: 0.0,
            longitude_deg: 0.0,
        },
        ObservationTime::local(1_000),
        FieldQuality::new(Some(12), Some(9), None, None),
        provenance(namespace),
    ));
    snapshot.position_status = PositionStatus::Current;
    let mut output = [0xa5_u8; GDL90_TRAFFIC_REPORT_LEN];

    let error = encode_traffic_report(&snapshot, TrafficAlertStatus::NoAlert, &mut output)
        .expect_err("GDL 90 leaves NIC values 12 through 15 unused");

    assert_eq!(
        error,
        TrafficReportError::Integrity {
            field: "nic",
            value: 12,
        }
    );
    assert_eq!(output, [0xa5; GDL90_TRAFFIC_REPORT_LEN]);
}

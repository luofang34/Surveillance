#![allow(clippy::expect_used, clippy::panic)]

use crate::source::{Band, SourceRef};

use crate::observation::{
    Callsign, InvalidObservation, InvalidPosition, InvalidSquawk, InvalidVelocity,
    ObservationOrigin, ObservationTime, PositionObservation, QualityIndicator, Squawk,
    TrafficObservation, VelocityObservation, Wgs84Position,
};
use crate::track::{AddressNamespace, TrackKey};

#[test]
fn squawk_accepts_four_octal_digits() {
    for code in [0, 1200, 7000, 7500, 7600, 7700, 7777] {
        let squawk = Squawk::try_from(code).expect("valid Mode A code");
        assert_eq!(squawk.code(), code);
        assert_eq!(u16::from(squawk), code);
    }
}

#[test]
fn squawk_rejects_non_octal_digits_and_overflow() {
    for code in [8, 79, 780, 7778, 7800, 8000, 9999, u16::MAX] {
        assert_eq!(Squawk::try_from(code), Err(InvalidSquawk { code }));
    }
}

#[test]
fn an_origin_reports_its_band_only_when_a_radio_received_it() {
    assert_eq!(
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090
        }
        .band(),
        Some(Band::Adsb1090)
    );
    assert_eq!(ObservationOrigin::ModeSReply.band(), Some(Band::Adsb1090));
    assert_eq!(
        ObservationOrigin::Tisb { band: Band::Uat978 }.band(),
        Some(Band::Uat978)
    );

    for originless in [
        ObservationOrigin::Radar,
        ObservationOrigin::Mlat,
        ObservationOrigin::ProviderFused,
        ObservationOrigin::PanelFused,
        ObservationOrigin::Replay,
        ObservationOrigin::Unknown,
    ] {
        assert_eq!(
            originless.band(),
            None,
            "{originless:?} never touched a radio in this runtime"
        );
    }
}

#[test]
fn an_origin_distinguishes_the_target_from_what_ground_derived() {
    assert!(
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090
        }
        .is_from_the_target()
    );
    assert!(ObservationOrigin::ModeSReply.is_from_the_target());
    assert!(
        ObservationOrigin::AdsR {
            band: Band::Adsb1090
        }
        .is_from_the_target(),
        "a rebroadcast still carries the target's own transmission"
    );

    assert!(!ObservationOrigin::Radar.is_from_the_target());
    assert!(!ObservationOrigin::Tisb { band: Band::Uat978 }.is_from_the_target());
    assert!(!ObservationOrigin::ProviderFused.is_from_the_target());

    assert!(ObservationOrigin::Tisb { band: Band::Uat978 }.is_ground_derived());
    assert!(ObservationOrigin::Radar.is_ground_derived());
    assert!(ObservationOrigin::Mlat.is_ground_derived());
    assert!(
        !ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090
        }
        .is_ground_derived()
    );
}

#[test]
fn new_observation_reports_no_measured_fields() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
    let observation = TrafficObservation::new(
        ObservationTime::local(5_000),
        SourceRef::default(),
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
        key,
    );

    assert_eq!(observation.key, key);
    assert_eq!(observation.position, None);
    assert_eq!(observation.pressure_altitude_ft, None);
    assert_eq!(observation.geometric_altitude_ft, None);
    assert_eq!(observation.velocity, None);
    assert_eq!(observation.callsign, None);
    assert_eq!(observation.squawk, None);
    assert_eq!(observation.air_ground, None);
    assert_eq!(observation.emergency, None);
    assert_eq!(observation.quality.nic, None);
}

#[test]
fn callsign_defaults_to_unknown_kind() {
    let callsign = Callsign::new("DAL123");
    assert_eq!(callsign.text, "DAL123");
    assert_eq!(callsign.kind, None);
}

#[test]
fn geographic_positions_reject_non_finite_and_out_of_range_values() {
    for (position, expected) in [
        (
            Wgs84Position {
                latitude_deg: f64::NAN,
                longitude_deg: 0.0,
            },
            InvalidPosition::LatitudeNotFinite { value: f64::NAN },
        ),
        (
            Wgs84Position {
                latitude_deg: 0.0,
                longitude_deg: f64::INFINITY,
            },
            InvalidPosition::LongitudeNotFinite {
                value: f64::INFINITY,
            },
        ),
        (
            Wgs84Position {
                latitude_deg: 90.1,
                longitude_deg: 0.0,
            },
            InvalidPosition::LatitudeRange { value: 90.1 },
        ),
        (
            Wgs84Position {
                latitude_deg: 0.0,
                longitude_deg: -180.1,
            },
            InvalidPosition::LongitudeRange { value: -180.1 },
        ),
    ] {
        let error = position.validate().expect_err("position must be invalid");
        if matches!(expected, InvalidPosition::LatitudeNotFinite { .. }) {
            assert!(matches!(error, InvalidPosition::LatitudeNotFinite { .. }));
        } else {
            assert_eq!(error, expected);
        }
    }
}

#[test]
fn invalid_canonical_values_are_rejected_before_ingest() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
    let mut observation = TrafficObservation::new(
        ObservationTime::local(1_000),
        SourceRef::default(),
        ObservationOrigin::ModeSReply,
        key,
    );
    observation.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 0.0,
        longitude_deg: 181.0,
    }));
    assert!(matches!(
        observation.validate(),
        Err(InvalidObservation::Position { .. })
    ));

    observation.position = None;
    observation.velocity = Some(VelocityObservation {
        ground_speed_kt: Some(-1.0),
        ..VelocityObservation::default()
    });
    assert_eq!(
        observation.validate(),
        Err(InvalidObservation::Velocity {
            source: InvalidVelocity::GroundSpeed { value: -1.0 }
        })
    );

    observation.velocity = None;
    observation.quality.nic = Some(12);
    assert_eq!(
        observation.validate(),
        Err(InvalidObservation::QualityRange {
            indicator: QualityIndicator::Nic,
            value: 12,
            maximum: 11,
        })
    );

    observation.quality.nic = None;
    observation.key = TrackKey::new(AddressNamespace::Icao, 0x0100_0000);
    assert_eq!(
        observation.validate(),
        Err(InvalidObservation::AddressRange {
            address: 0x0100_0000
        })
    );
}

#[test]
fn each_floating_velocity_class_rejects_invalid_values() {
    let mut velocity = VelocityObservation {
        ground_speed_kt: Some(f64::NEG_INFINITY),
        ..VelocityObservation::default()
    };
    assert!(matches!(
        velocity.validate(),
        Err(InvalidVelocity::GroundSpeed { .. })
    ));

    velocity = VelocityObservation {
        track_angle_deg_true: Some(360.0),
        ..VelocityObservation::default()
    };
    assert_eq!(
        velocity.validate(),
        Err(InvalidVelocity::TrackAngle { value: 360.0 })
    );

    velocity = VelocityObservation {
        airspeed_kt: Some(f64::NAN),
        ..VelocityObservation::default()
    };
    assert!(matches!(
        velocity.validate(),
        Err(InvalidVelocity::Airspeed { .. })
    ));

    velocity = VelocityObservation {
        heading_deg: Some(-0.1),
        ..VelocityObservation::default()
    };
    assert_eq!(
        velocity.validate(),
        Err(InvalidVelocity::Heading { value: -0.1 })
    );
}

#[test]
fn every_quality_indicator_obeys_its_defined_range() {
    let key = TrackKey::new(AddressNamespace::Icao, 1);
    for (quality, indicator, value, maximum) in [
        (
            crate::FieldQuality::new(Some(12), None, None, None),
            QualityIndicator::Nic,
            12,
            11,
        ),
        (
            crate::FieldQuality::new(None, Some(12), None, None),
            QualityIndicator::NacP,
            12,
            11,
        ),
        (
            crate::FieldQuality::new(None, None, Some(5), None),
            QualityIndicator::NacV,
            5,
            4,
        ),
        (
            crate::FieldQuality::new(None, None, None, Some(4)),
            QualityIndicator::Sil,
            4,
            3,
        ),
    ] {
        let mut observation = TrafficObservation::new(
            ObservationTime::local(1_000),
            SourceRef::default(),
            ObservationOrigin::Unknown,
            key,
        );
        observation.quality = quality;
        assert_eq!(
            observation.validate(),
            Err(InvalidObservation::QualityRange {
                indicator,
                value,
                maximum,
            })
        );
    }
}

#[test]
fn a_public_cpr_literal_still_receives_boundary_validation() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
    let mut observation = TrafficObservation::new(
        ObservationTime::local(1_000),
        SourceRef::default(),
        ObservationOrigin::ModeSReply,
        key,
    );
    observation.position = Some(PositionObservation::CprAirborne(crate::CprFrame {
        format: crate::CprFormat::Even,
        encoded_latitude: crate::CprFrame::MAX_ENCODED + 1,
        encoded_longitude: 0,
    }));

    assert!(matches!(
        observation.validate(),
        Err(InvalidObservation::CprFrame { .. })
    ));
}

#[test]
fn time_basis_invariants_apply_to_public_literals() {
    let observation = TrafficObservation::new(
        ObservationTime {
            received_at_micros: 1_000,
            observed_at_micros: 1_001,
            basis: crate::TimeBasis::Mapped {
                uncertainty_micros: 0,
            },
        },
        SourceRef::default(),
        ObservationOrigin::Unknown,
        TrackKey::new(AddressNamespace::Icao, 1),
    );

    assert!(matches!(
        observation.validate(),
        Err(InvalidObservation::ObservationAfterReception { .. })
    ));

    let mismatched = TrafficObservation::new(
        ObservationTime {
            received_at_micros: 1_000,
            observed_at_micros: 999,
            basis: crate::TimeBasis::LocalIngress,
        },
        SourceRef::default(),
        ObservationOrigin::Unknown,
        TrackKey::new(AddressNamespace::Icao, 1),
    );
    assert_eq!(
        mismatched.validate(),
        Err(InvalidObservation::TimeBasisMismatch {
            basis: crate::TimeBasis::LocalIngress
        })
    );
}

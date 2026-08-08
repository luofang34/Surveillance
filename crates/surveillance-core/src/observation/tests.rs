#![allow(clippy::expect_used, clippy::panic)]

use crate::source::{Band, SourceRef};

use crate::observation::{
    Callsign, InvalidSquawk, ObservationOrigin, ObservationTime, Squawk, TrafficObservation,
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

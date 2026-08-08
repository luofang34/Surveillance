#![allow(clippy::expect_used, clippy::panic)]

use crate::source::{Band, SourceRef};

use crate::observation::{Callsign, InvalidSquawk, Squawk, TrafficLink, TrafficObservation};
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
fn traffic_link_reports_band_and_rebroadcast() {
    assert_eq!(TrafficLink::Adsb1090.band(), Band::Adsb1090);
    assert_eq!(TrafficLink::ModeS.band(), Band::Adsb1090);
    assert_eq!(TrafficLink::Uat978.band(), Band::Uat978);
    assert_eq!(
        TrafficLink::AdsR { band: Band::Uat978 }.band(),
        Band::Uat978
    );
    assert_eq!(
        TrafficLink::Tisb {
            band: Band::Adsb1090
        }
        .band(),
        Band::Adsb1090
    );

    assert!(!TrafficLink::Adsb1090.is_rebroadcast());
    assert!(!TrafficLink::Uat978.is_rebroadcast());
    assert!(
        TrafficLink::AdsR {
            band: Band::Adsb1090
        }
        .is_rebroadcast()
    );
    assert!(TrafficLink::Tisb { band: Band::Uat978 }.is_rebroadcast());
}

#[test]
fn new_observation_reports_no_measured_fields() {
    let key = TrackKey::new(AddressNamespace::Icao, 0x00AC_82EC);
    let observation =
        TrafficObservation::new(5_000, SourceRef::default(), TrafficLink::Adsb1090, key);

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

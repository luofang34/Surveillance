#![allow(clippy::expect_used, clippy::panic)]

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use aero_link::{
    AddressQualifier, Adsb1090Message, AirGroundState, Altitude, AltitudeKind, CorrectedUatFrame,
    GeoPosition, ReceiverSource, ReceptionEvent, StateVector, UatDecoder, UatDownlink,
    UatFrameClass, UatMessage, UatReception,
};

use crate::NormalizeError;
use surveillance_core::{
    AddressNamespace, PositionObservation, SourceRef, SourceTransport, TrafficLink,
    TrafficObservation,
};

/// A published DF 17 airborne-position frame (even half of a known pair).
const DF17_AIRBORNE_POSITION: &str = "8D40621D58C382D690C8AC2863A7";
/// A published DF 17 aircraft-identification frame.
const DF17_IDENTIFICATION: &str = "8D4840D6202CC371C32CE0576098";

fn decode(hex: &str) -> Adsb1090Message {
    let bytes = hex_bytes(hex);
    Adsb1090Message::decode(&bytes, None).expect("published frame decodes")
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|index| {
            u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("valid hex pair")
        })
        .collect()
}

fn receiver() -> ReceiverSource {
    ReceiverSource::new(3, aero_link::SourceTransport::Usb)
}

/// The canonical source the adapter must produce from `receiver()`.
fn expected_source() -> SourceRef {
    SourceRef::new(3, SourceTransport::Usb)
}

fn normalize_1090(hex: &str, at: u64) -> Result<TrafficObservation, NormalizeError> {
    let event = ReceptionEvent::adsb1090(at, decode(hex)).with_source(receiver());
    let projection = event.traffic().expect("a 1090 reception is traffic");
    crate::normalize(&projection)
}

fn corrected(class: UatFrameClass) -> CorrectedUatFrame {
    let bytes: Box<[u8]> = vec![0_u8; class.corrected_bytes()].into_boxed_slice();
    CorrectedUatFrame::try_from_boxed_bytes(class, bytes).expect("a sized frame is valid")
}

fn uat_event(at: u64, downlink: UatDownlink) -> ReceptionEvent {
    let reception = UatReception {
        rssi_raw: -20,
        receiver_timestamp: 0,
        corrected_symbols: 0,
        encoded_frame: None,
        corrected_frame: corrected(UatFrameClass::BasicDownlink),
        message: UatMessage::Downlink(downlink),
    };
    ReceptionEvent::uat978(at, reception).with_source(receiver())
}

fn uat_downlink(qualifier: AddressQualifier, address: u32) -> UatDownlink {
    UatDownlink {
        message_type: 1,
        address_qualifier: qualifier,
        address,
        state_vector: None,
        mode_status: None,
        secondary_altitude: None,
    }
}

#[test]
fn a_direct_airborne_position_normalizes_to_an_icao_key_and_a_cpr_frame() {
    let observation = normalize_1090(DF17_AIRBORNE_POSITION, 5_000).expect("frame normalizes");

    assert_eq!(observation.received_at_micros, 5_000);
    assert_eq!(observation.source, expected_source());
    assert_eq!(observation.link, TrafficLink::Adsb1090);
    assert_eq!(observation.key.namespace, AddressNamespace::Icao);
    assert_eq!(observation.key.address, 0x0040_621D);

    let Some(PositionObservation::CprAirborne(frame)) = observation.position else {
        panic!(
            "expected an undecoded airborne CPR frame, got {:?}",
            observation.position
        );
    };
    assert_eq!(frame.encoded_latitude, 93_000);
    assert_eq!(frame.encoded_longitude, 51_372);
    assert!(
        observation.pressure_altitude_ft.is_some(),
        "the frame carries a barometric altitude"
    );
    assert_eq!(
        observation.quality.nic, None,
        "a position message carries a NIC supplement bit, not a NIC category"
    );
}

#[test]
fn an_identification_frame_supplies_a_callsign_and_emitter_category() {
    let observation = normalize_1090(DF17_IDENTIFICATION, 1_000).expect("frame normalizes");

    let callsign = observation.callsign.as_ref().expect("callsign decoded");
    assert!(
        !callsign.text.is_empty(),
        "a decoded identification carries a callsign"
    );
    assert!(observation.emitter_category.is_some());
    assert_eq!(
        observation.position, None,
        "an identification frame reports no position"
    );
}

#[test]
fn a_non_traffic_reception_never_reaches_normalization() {
    let reception = UatDecoder::default()
        .decode_corrected(corrected(UatFrameClass::GroundUplink), -20, 0, 0)
        .expect("a ground uplink frame decodes");
    assert!(
        matches!(reception.message, UatMessage::Uplink(_)),
        "a 432-byte frame is a ground uplink"
    );
    let uplink = ReceptionEvent::uat978(1_000, reception);
    assert!(
        uplink.traffic().is_none(),
        "a ground uplink is not a traffic reception"
    );
}

#[test]
fn each_uat_qualifier_selects_its_namespace_and_link() {
    let cases = [
        (
            AddressQualifier::AdsbIcao,
            AddressNamespace::Icao,
            TrafficLink::Uat978,
        ),
        (
            AddressQualifier::National,
            AddressNamespace::AdsbNonIcao,
            TrafficLink::Uat978,
        ),
        (
            AddressQualifier::TisbIcao,
            AddressNamespace::Icao,
            TrafficLink::Tisb {
                band: surveillance_core::Band::Uat978,
            },
        ),
        (
            AddressQualifier::TisbTrackFile,
            AddressNamespace::TisbTrackFile,
            TrafficLink::Tisb {
                band: surveillance_core::Band::Uat978,
            },
        ),
        (
            AddressQualifier::Vehicle,
            AddressNamespace::SurfaceVehicle,
            TrafficLink::Uat978,
        ),
        (
            AddressQualifier::FixedBeacon,
            AddressNamespace::FixedBeacon,
            TrafficLink::Uat978,
        ),
    ];

    for (qualifier, namespace, link) in cases {
        let event = uat_event(2_000, uat_downlink(qualifier, 0x00AB_CDEF));
        let projection = event.traffic().expect("a UAT downlink is traffic");
        let observation = crate::normalize(&projection).expect("downlink normalizes");

        assert_eq!(observation.key.namespace, namespace, "for {qualifier:?}");
        assert_eq!(observation.link, link, "for {qualifier:?}");
        assert_eq!(observation.key.address, 0x00AB_CDEF);
    }
}

#[test]
fn a_reserved_qualifier_is_refused_rather_than_guessed() {
    for (qualifier, expected) in [
        (AddressQualifier::Reserved6, 6),
        (AddressQualifier::Reserved7, 7),
    ] {
        let event = uat_event(2_000, uat_downlink(qualifier, 1));
        let projection = event.traffic().expect("a UAT downlink is traffic");
        assert_eq!(
            crate::normalize(&projection),
            Err(NormalizeError::ReservedAddressQualifier {
                qualifier: expected
            })
        );
    }
}

#[test]
fn equal_addresses_under_different_qualifiers_normalize_to_distinct_keys() {
    let mut keys = Vec::new();
    for qualifier in [
        AddressQualifier::AdsbIcao,
        AddressQualifier::TisbIcao,
        AddressQualifier::TisbTrackFile,
        AddressQualifier::Vehicle,
        AddressQualifier::FixedBeacon,
    ] {
        let event = uat_event(2_000, uat_downlink(qualifier, 0x0000_00AA));
        let projection = event.traffic().expect("a UAT downlink is traffic");
        keys.push(
            crate::normalize(&projection)
                .expect("downlink normalizes")
                .key,
        );
    }

    let mut unique = keys.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        keys.len() - 1,
        "only the two ICAO qualifiers share a namespace"
    );
}

#[test]
fn a_uat_state_vector_supplies_a_decoded_position_in_explicit_units() {
    let mut downlink = uat_downlink(AddressQualifier::AdsbIcao, 0x00AC_82EC);
    downlink.state_vector = Some(StateVector {
        navigation_integrity_category: 8,
        position: Some(GeoPosition {
            latitude: 40.639751,
            longitude: -73.778925,
        }),
        altitude: Some(Altitude {
            feet: 12_500,
            kind: AltitudeKind::Barometric,
        }),
        air_ground_state: AirGroundState::Subsonic,
        north_south_velocity_knots: None,
        east_west_velocity_knots: None,
        speed_knots: Some(214),
        heading_degrees: Some(87),
        heading_kind: Some(aero_link::HeadingKind::Track),
        vertical_rate_fpm: Some(-640),
        vertical_rate_source: Some(AltitudeKind::Barometric),
        length_metres: None,
        width_metres: None,
        position_offset_applied: false,
        utc_coupled: false,
        tisb_site_id: 0,
    });

    let event = uat_event(3_000, downlink);
    let projection = event.traffic().expect("a UAT downlink is traffic");
    let observation = crate::normalize(&projection).expect("downlink normalizes");

    let Some(PositionObservation::Decoded(position)) = observation.position else {
        panic!(
            "UAT positions arrive decoded, got {:?}",
            observation.position
        );
    };
    assert_eq!(position.latitude_deg, 40.639751);
    assert_eq!(position.longitude_deg, -73.778925);
    assert_eq!(observation.pressure_altitude_ft, Some(12_500));
    assert_eq!(observation.geometric_altitude_ft, None);
    assert_eq!(
        observation.air_ground,
        Some(surveillance_core::AirGroundState::Subsonic)
    );
    assert_eq!(observation.quality.nic, Some(8));

    let velocity = observation.velocity.expect("velocity applied");
    assert_eq!(velocity.ground_speed_kt, Some(214.0));
    assert_eq!(velocity.track_angle_deg_true, Some(87.0));
    assert_eq!(
        velocity.heading_deg, None,
        "a track report is not a heading report"
    );
    assert_eq!(velocity.vertical_rate_fpm, Some(-640));
}

#[test]
fn a_downlink_without_optional_blocks_leaves_every_field_absent() {
    let event = uat_event(4_000, uat_downlink(AddressQualifier::AdsbIcao, 0x00AC_82EC));
    let projection = event.traffic().expect("a UAT downlink is traffic");
    let observation = crate::normalize(&projection).expect("downlink normalizes");

    assert_eq!(observation.position, None);
    assert_eq!(observation.pressure_altitude_ft, None);
    assert_eq!(observation.geometric_altitude_ft, None);
    assert_eq!(observation.velocity, None);
    assert_eq!(observation.callsign, None);
    assert_eq!(observation.squawk, None);
    assert_eq!(observation.air_ground, None);
    assert_eq!(observation.emergency, None);
    assert_eq!(observation.emitter_category, None);
    assert_eq!(observation.quality.nic, None);
    assert_eq!(observation.quality.nac_p, None);
}

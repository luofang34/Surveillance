//! Coverage for the link classifications a live receiver actually produces.
//!
//! Frames are synthesized rather than captured, because the interesting
//! classifications — ADS-R, 1090 TIS-B, coarse TIS-B track files, and the
//! DF 18 control-field edges — are rare in any one capture and impossible to
//! provoke on demand. The parity helper that makes synthesis possible is
//! itself validated against published frames before anything relies on it.

#![allow(clippy::expect_used, clippy::panic)]

use aero_link::{Adsb1090Message, ReceiverSource, ReceptionEvent, SourceTransport};
use surveillance_aero_link::{NormalizeError, normalize};
use surveillance_core::{
    AddressNamespace, Band, HeadingReference, ObservationOrigin, TrafficObservation,
};

/// Mode S 24-bit parity over a message whose parity field is zero.
fn parity(payload: &[u8]) -> u32 {
    let mut data = payload.to_vec();
    data.extend_from_slice(&[0, 0, 0]);
    let mut crc: u32 = 0;
    for index in 0..data.len() * 8 {
        let bit = (data[index / 8] >> (7 - (index % 8))) & 1;
        crc = ((crc << 1) | u32::from(bit)) & 0x01FF_FFFF;
        if crc & 0x0100_0000 != 0 {
            crc ^= 0x01FF_F409;
        }
    }
    crc & 0x00FF_FFFF
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|index| {
            u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("valid hex pair")
        })
        .collect()
}

/// Append correct parity to an 11-byte extended-squitter payload.
fn with_parity(payload: [u8; 11]) -> Vec<u8> {
    let checksum = parity(&payload);
    let mut frame = payload.to_vec();
    frame.push((checksum >> 16) as u8);
    frame.push((checksum >> 8) as u8);
    frame.push(checksum as u8);
    frame
}

/// A DF 18 frame with the given control field, address, and message element.
fn df18(control_field: u8, address: u32, message: [u8; 7]) -> Vec<u8> {
    let mut payload = [0_u8; 11];
    payload[0] = (18 << 3) | (control_field & 0x07);
    payload[1] = (address >> 16) as u8;
    payload[2] = (address >> 8) as u8;
    payload[3] = address as u8;
    payload[4..11].copy_from_slice(&message);
    with_parity(payload)
}

/// An identification message element, whose type code fixes IMF to false.
fn identification() -> [u8; 7] {
    let mut message = [0_u8; 7];
    message[0] = 1 << 3;
    message
}

fn observe(frame: &[u8]) -> Result<TrafficObservation, NormalizeError> {
    let message = Adsb1090Message::decode(frame, None).expect("synthesized frame decodes");
    let event = ReceptionEvent::adsb1090(1_000, message)
        .with_source(ReceiverSource::new(1, SourceTransport::Usb));
    let projection = event.traffic().expect("a 1090 reception is traffic");
    normalize(&projection)
}

#[test]
fn the_parity_helper_reproduces_published_frames() {
    for published in [
        "8D40621D58C382D690C8AC2863A7",
        "8D4840D6202CC371C32CE0576098",
        "8D40621D58C386435CC412692AD6",
    ] {
        let bytes = hex_bytes(published);
        let (payload, expected) = bytes.split_at(11);
        assert_eq!(
            parity(payload).to_be_bytes()[1..],
            *expected,
            "parity must match {published} before any synthesized frame relies on it"
        );
    }
}

#[test]
fn an_adsr_rebroadcast_keeps_the_icao_namespace_and_names_its_origin() {
    let observation =
        observe(&df18(6, 0x00AB_CDEF, identification())).expect("an ADS-R rebroadcast normalizes");

    assert_eq!(observation.key.namespace, AddressNamespace::Icao);
    assert_eq!(observation.key.address, 0x00AB_CDEF);
    assert_eq!(
        observation.origin,
        ObservationOrigin::AdsR {
            band: Band::Adsb1090
        },
        "a rebroadcast must not claim to have come straight from the target"
    );
}

#[test]
fn fine_tisb_with_an_icao_address_reaches_the_icao_namespace() {
    let observation =
        observe(&df18(2, 0x00AB_CDEF, identification())).expect("fine TIS-B normalizes");

    assert_eq!(observation.key.namespace, AddressNamespace::Icao);
    assert_eq!(
        observation.origin,
        ObservationOrigin::Tisb {
            band: Band::Adsb1090
        }
    );
}

#[test]
fn fine_tisb_without_an_icao_address_becomes_a_track_file() {
    let observation =
        observe(&df18(5, 0x0000_0042, identification())).expect("non-ICAO fine TIS-B normalizes");

    assert_eq!(observation.key.namespace, AddressNamespace::TisbTrackFile);
    assert_eq!(observation.key.address, 0x0000_0042);
}

#[test]
fn a_coarse_tisb_track_file_packs_its_mode_a_code_and_file_number() {
    // Coarse TIS-B takes IMF from the first message bit; setting it selects
    // the Mode A track-file interpretation of the address field.
    let mut message = [0_u8; 7];
    message[0] = 0x80;
    // The address field is 24 bits: a Mode A code of 0o1064 in the high
    // twelve and track file 0x567 in the low twelve.
    let observation = observe(&df18(3, 0x0023_4567, message)).expect("coarse TIS-B normalizes");

    assert_eq!(
        observation.key.namespace,
        AddressNamespace::TisbModeATrackFile,
        "a Mode A track file is its own identity space"
    );
    assert_eq!(observation.key.address, 0x0023_4567);
    assert_eq!(
        observation.key.address >> 12,
        0x234,
        "the Mode A code must be recoverable from the packed key"
    );
    assert_eq!(
        observation.key.address & 0x0FFF,
        0x567,
        "and so must the track-file number"
    );
}

#[test]
fn an_anonymous_address_stays_unclassified_until_a_category_arrives() {
    let observation =
        observe(&df18(1, 0x00AB_CDEF, identification())).expect("anonymous ADS-B normalizes");

    assert_eq!(
        observation.key.namespace,
        AddressNamespace::AdsbNonIcao,
        "an address that may yet be a vehicle is never treated as an aircraft"
    );
    assert_eq!(
        observation.origin,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090
        }
    );
}

#[test]
fn service_management_data_is_refused_rather_than_tracked() {
    assert_eq!(
        observe(&df18(4, 0x00AB_CDEF, identification())),
        Err(NormalizeError::ServiceManagement)
    );
}

/// A surveillance reply carries no address of its own: the receiver recovers
/// one by XORing the announced address back out of the parity field.
fn surveillance_reply(downlink_format: u8, address: u32, body: [u8; 3]) -> Vec<u8> {
    let mut payload = [0_u8; 4];
    payload[0] = downlink_format << 3;
    payload[1..4].copy_from_slice(&body);
    let overlaid = parity(&payload) ^ address;
    let mut frame = payload.to_vec();
    frame.push((overlaid >> 16) as u8);
    frame.push((overlaid >> 8) as u8);
    frame.push(overlaid as u8);
    frame
}

#[test]
fn an_unconfirmed_parity_recovered_address_is_refused() {
    let frame = surveillance_reply(4, 0x00AB_CDEF, [0x00, 0x02, 0x30]);
    let message = Adsb1090Message::decode(&frame, None).expect("a DF 4 reply decodes");
    let event = ReceptionEvent::adsb1090(1_000, message);
    let projection = event.traffic().expect("a 1090 reception is traffic");

    assert_eq!(
        normalize(&projection),
        Err(NormalizeError::UnconfirmedAddress { downlink_format: 4 }),
        "a candidate no receiver has independently seen must not create a track"
    );
}

#[test]
fn a_confirmed_reply_supplies_altitude_under_a_mode_s_origin() {
    // An altitude code encoding 2600 ft pressure altitude.
    let frame = surveillance_reply(4, 0x00AB_CDEF, [0x00, 0x02, 0x30]);
    let mut message = Adsb1090Message::decode(&frame, None).expect("a DF 4 reply decodes");
    message.frame.confirm_address_parity();

    let event = ReceptionEvent::adsb1090(1_000, message);
    let projection = event.traffic().expect("a 1090 reception is traffic");
    let observation = normalize(&projection).expect("a confirmed reply normalizes");

    assert_eq!(observation.key.namespace, AddressNamespace::Icao);
    assert_eq!(observation.key.address, 0x00AB_CDEF);
    assert_eq!(
        observation.origin,
        ObservationOrigin::ModeSReply,
        "a reply comes from the target but carries neither position nor integrity"
    );
    assert_eq!(
        observation.pressure_altitude_ft,
        Some(2_600),
        "a DF 4 reply's altitude code is the whole reason to ingest it"
    );
    assert_eq!(observation.position, None);
    assert_eq!(observation.quality.nic, None);
}

#[test]
fn a_confirmed_identity_reply_supplies_a_squawk() {
    // Mode A 1200 in the identity field of a DF 5 reply.
    let frame = surveillance_reply(5, 0x00AB_CDEF, [0x00, 0x0A, 0x00]);
    let mut message = Adsb1090Message::decode(&frame, None).expect("a DF 5 reply decodes");
    message.frame.confirm_address_parity();

    let event = ReceptionEvent::adsb1090(2_000, message);
    let projection = event.traffic().expect("a 1090 reception is traffic");
    let observation = normalize(&projection).expect("a confirmed reply normalizes");

    assert_eq!(observation.origin, ObservationOrigin::ModeSReply);
    assert!(observation.squawk.is_some());
    assert_eq!(observation.pressure_altitude_ft, None);
}

/// Velocity subtypes 1 and 2 report a ground track; 3 and 4 report a magnetic
/// heading. Storing the first as a heading would label a track angle as
/// something a consumer steers by.
#[test]
fn a_ground_velocity_reports_a_track_angle_and_no_heading() {
    let mut message = [0_u8; 7];
    message[0] = (19 << 3) | 1;
    message[2] = 0x02;
    message[3] = 0x01;
    message[4] = 0x02;
    message[5] = 0x01;
    let observation =
        observe(&df18(0, 0x00AB_CDEF, message)).expect("a ground velocity normalizes");

    let velocity = observation.velocity.expect("velocity applied");
    assert_eq!(
        velocity.heading_deg, None,
        "a ground velocity reports no heading"
    );
    assert_eq!(velocity.heading_reference, None);
    assert!(
        velocity.track_angle_deg_true.is_some(),
        "the track angle is derived from the velocity components"
    );
}

#[test]
fn an_airspeed_velocity_reports_a_magnetic_heading() {
    // The airspeed subtypes replace the velocity components with a heading
    // status bit, ten heading bits, and an airspeed. Heading is only present
    // when the status bit says so, which is the case worth covering.
    let mut message = [0_u8; 7];
    message[0] = (19 << 3) | 3;
    message[1] = 0x05;
    message[2] = 0x00;
    message[3] = 0x40;
    let observation =
        observe(&df18(0, 0x00AB_CDEF, message)).expect("an airspeed velocity normalizes");

    let velocity = observation.velocity.expect("velocity applied");
    assert!(
        velocity.heading_deg.is_some(),
        "an airspeed subtype reports a heading"
    );
    assert_eq!(
        velocity.heading_reference,
        Some(HeadingReference::MagneticNorth),
        "and it is measured from magnetic north, not true"
    );
    assert_eq!(
        velocity.track_angle_deg_true, None,
        "a heading is not a track angle"
    );
}

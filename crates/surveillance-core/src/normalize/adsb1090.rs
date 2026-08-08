//! Normalizing 1090 MHz Mode S and extended-squitter receptions.

use aero_link::{
    Adsb1090Message, AltitudeKind, Band, ExtendedSquitter, ExtendedSquitterAddress,
    ExtendedSquitterMessage, IcaoAddressStatus, ModeSMessage, TrafficProjection,
};

use crate::cpr::{CprFormat, CprFrame};
use crate::normalize::NormalizeError;
use crate::observation::{
    Callsign, PositionObservation, Squawk, TrafficLink, TrafficObservation, VelocityObservation,
};
use crate::track::{AddressNamespace, TrackKey};

/// Normalize one 1090 MHz reception.
pub(super) fn normalize(
    projection: &TrafficProjection<'_>,
    message: &Adsb1090Message,
) -> Result<TrafficObservation, NormalizeError> {
    let frame = &message.frame;
    match &frame.message {
        ModeSMessage::ExtendedSquitter(squitter) => {
            let (key, link) = classify_squitter(squitter)?;
            let mut observation = TrafficObservation::new(
                projection.received_at_micros,
                projection.source,
                link,
                key,
            );
            apply_squitter(&mut observation, squitter)?;
            Ok(observation)
        }
        other => {
            let key = surveillance_key(
                frame.icao_address,
                frame.icao_address_status,
                frame.downlink_format,
            )?;
            let mut observation = TrafficObservation::new(
                projection.received_at_micros,
                projection.source,
                TrafficLink::ModeS,
                key,
            );
            apply_surveillance(&mut observation, other)?;
            Ok(observation)
        }
    }
}

/// Map an extended-squitter address onto a namespace and a link class.
///
/// The address variant already encodes both the namespace and the service
/// that produced the frame, so this reads the decoded classification rather
/// than re-deriving it from the control field.
fn classify_squitter(
    squitter: &ExtendedSquitter,
) -> Result<(TrackKey, TrafficLink), NormalizeError> {
    let tisb = TrafficLink::Tisb {
        band: Band::Adsb1090,
    };
    let adsr = TrafficLink::AdsR {
        band: Band::Adsb1090,
    };

    let (namespace, address, link) = match squitter.address {
        ExtendedSquitterAddress::AdsbIcao { address } => {
            (AddressNamespace::Icao, address, TrafficLink::Adsb1090)
        }
        ExtendedSquitterAddress::AdsbNonIcao { address } => (
            AddressNamespace::AdsbNonIcao,
            address,
            TrafficLink::Adsb1090,
        ),
        ExtendedSquitterAddress::TisbIcao { address } => (AddressNamespace::Icao, address, tisb),
        ExtendedSquitterAddress::TisbTrackFile { address } => {
            (AddressNamespace::TisbTrackFile, address, tisb)
        }
        ExtendedSquitterAddress::TisbModeATrackFile {
            mode_a_code,
            track_file_number,
        } => (
            AddressNamespace::TisbModeATrackFile,
            pack_mode_a_track_file(mode_a_code, track_file_number),
            tisb,
        ),
        ExtendedSquitterAddress::AdsrRelayedIcao { address } => {
            (AddressNamespace::Icao, address, adsr)
        }
        ExtendedSquitterAddress::AdsrRelayedNonIcao { address } => {
            (AddressNamespace::AdsbNonIcao, address, adsr)
        }
        _ => return Err(NormalizeError::ServiceManagement),
    };
    Ok((TrackKey::new(namespace, address), link))
}

/// Pack a Mode A code and track-file number into one key value.
///
/// Both fields are twelve bits, so the pair fits a `u32` without loss and
/// keeps the identity comparable, orderable, and serializable like every
/// other address.
fn pack_mode_a_track_file(mode_a_code: u16, track_file_number: u16) -> u32 {
    (u32::from(mode_a_code) << 12) | u32::from(track_file_number & 0x0FFF)
}

/// Key a Mode S surveillance reply, refusing an address no receiver has
/// confirmed.
fn surveillance_key(
    icao_address: Option<u32>,
    status: Option<IcaoAddressStatus>,
    downlink_format: u8,
) -> Result<TrackKey, NormalizeError> {
    let address = icao_address.ok_or(NormalizeError::NoTargetIdentity { downlink_format })?;
    match status {
        Some(IcaoAddressStatus::Announced | IcaoAddressStatus::AddressParityConfirmed) => {
            Ok(TrackKey::new(AddressNamespace::Icao, address))
        }
        _ => Err(NormalizeError::UnconfirmedAddress { downlink_format }),
    }
}

fn apply_squitter(
    observation: &mut TrafficObservation,
    squitter: &ExtendedSquitter,
) -> Result<(), NormalizeError> {
    match &squitter.message {
        ExtendedSquitterMessage::Identification(identification) => {
            observation.callsign = Some(Callsign::new(identification.callsign.as_str()));
            observation.emitter_category = Some(identification.emitter_category);
        }
        ExtendedSquitterMessage::AirbornePosition(position) => {
            observation.position = Some(PositionObservation::CprAirborne(cpr_frame(
                position.odd_cpr,
                position.cpr_latitude,
                position.cpr_longitude,
            )));
            apply_altitude(observation, position.altitude);
            observation.quality.nic = position.nic_supplement_b.map(u8::from);
        }
        ExtendedSquitterMessage::SurfacePosition(position) => {
            observation.position = Some(PositionObservation::CprSurface(cpr_frame(
                position.odd_cpr,
                position.cpr_latitude,
                position.cpr_longitude,
            )));
            observation.velocity = Some(VelocityObservation {
                ground_speed_kt: position.ground_speed_knots,
                track_angle_deg_true: position.ground_track_degrees,
                ..VelocityObservation::default()
            });
        }
        ExtendedSquitterMessage::AirborneVelocity(velocity) => {
            apply_velocity(observation, velocity);
        }
        ExtendedSquitterMessage::AircraftStatus(status) => {
            observation.emergency = Some(status.emergency_state);
            observation.squawk = Some(Squawk::try_from(status.squawk)?);
        }
        _ => {}
    }
    Ok(())
}

fn apply_velocity(observation: &mut TrafficObservation, velocity: &aero_link::AirborneVelocity) {
    let ground_speed_kt = match (
        velocity.east_west_velocity_knots,
        velocity.north_south_velocity_knots,
    ) {
        (Some(east_west), Some(north_south)) => {
            let east_west = f64::from(east_west);
            let north_south = f64::from(north_south);
            Some(libm::sqrt(
                east_west * east_west + north_south * north_south,
            ))
        }
        _ => None,
    };
    let track_angle_deg_true = match (
        velocity.east_west_velocity_knots,
        velocity.north_south_velocity_knots,
    ) {
        (Some(east_west), Some(north_south)) => Some(bearing_degrees(
            f64::from(east_west),
            f64::from(north_south),
        )),
        _ => None,
    };

    observation.velocity = Some(VelocityObservation {
        ground_speed_kt,
        track_angle_deg_true,
        airspeed_kt: velocity.airspeed_knots.map(f64::from),
        airspeed_kind: velocity.airspeed_kind,
        heading_deg: velocity.heading_degrees.map(f64::from),
        heading_reference: None,
        vertical_rate_fpm: velocity.vertical_rate_fpm.map(i32::from),
        vertical_rate_source: velocity.vertical_rate_source,
    });
    observation.quality.nac_v = Some(velocity.navigation_accuracy_category_velocity);
}

/// Convert an east/north velocity pair into a true track angle clockwise from
/// north, which is the convention every consumer of the exported schema uses.
fn bearing_degrees(east_west_kt: f64, north_south_kt: f64) -> f64 {
    let degrees = libm::atan2(east_west_kt, north_south_kt).to_degrees();
    if degrees < 0.0 {
        degrees + 360.0
    } else {
        degrees
    }
}

fn apply_altitude(observation: &mut TrafficObservation, altitude: Option<aero_link::Altitude>) {
    let Some(altitude) = altitude else {
        return;
    };
    match altitude.kind {
        AltitudeKind::Barometric => observation.pressure_altitude_ft = Some(altitude.feet),
        AltitudeKind::Geometric => observation.geometric_altitude_ft = Some(altitude.feet),
    }
}

fn apply_surveillance(
    observation: &mut TrafficObservation,
    message: &ModeSMessage,
) -> Result<(), NormalizeError> {
    match message {
        ModeSMessage::SurveillanceAltitudeReply(reply) => {
            apply_altitude(observation, reply.altitude)
        }
        ModeSMessage::CommBAltitudeReply(reply) => apply_altitude(observation, reply.altitude),
        ModeSMessage::ShortAirAirReply(reply) => apply_altitude(observation, reply.altitude),
        ModeSMessage::LongAirAirReply(reply) => apply_altitude(observation, reply.altitude),
        ModeSMessage::SurveillanceIdentityReply(reply) => {
            observation.squawk = Some(Squawk::try_from(reply.squawk)?);
        }
        ModeSMessage::CommBIdentityReply(reply) => {
            observation.squawk = Some(Squawk::try_from(reply.squawk)?);
        }
        _ => {}
    }
    Ok(())
}

fn cpr_frame(odd: bool, encoded_latitude: u32, encoded_longitude: u32) -> CprFrame {
    let format = if odd { CprFormat::Odd } else { CprFormat::Even };
    CprFrame {
        format,
        encoded_latitude,
        encoded_longitude,
    }
}

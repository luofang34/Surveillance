//! Normalizing 1090 MHz Mode S and extended-squitter receptions.

use aero_link::{
    Adsb1090Message, AltitudeKind, ExtendedSquitter, ExtendedSquitterAddress,
    ExtendedSquitterMessage, IcaoAddressStatus, ModeSMessage, OperationalStatus, TrafficProjection,
};
use surveillance_core::{
    AddressNamespace, AdsbIntegrityDeclaration, AdsbPositionIntegrity, AdsbVersion, Band, Callsign,
    CprFormat, CprFrame, EmitterCategory, HeadingReference, ObservationOrigin, ObservationTime,
    PositionObservation, Squawk, TrackKey, TrafficObservation, VelocityObservation,
};

use crate::NormalizeError;
use crate::source::source_ref;
use crate::vocabulary;

/// Normalize one 1090 MHz reception.
pub(crate) fn normalize(
    projection: &TrafficProjection<'_>,
    message: &Adsb1090Message,
) -> Result<TrafficObservation, NormalizeError> {
    let frame = &message.frame;
    match &frame.message {
        ModeSMessage::ExtendedSquitter(squitter) => {
            let (key, link) = classify_squitter(squitter)?;
            let mut observation = TrafficObservation::new(
                ObservationTime::local(projection.received_at_micros),
                source_ref(projection.source),
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
                ObservationTime::local(projection.received_at_micros),
                source_ref(projection.source),
                ObservationOrigin::ModeSReply,
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
) -> Result<(TrackKey, ObservationOrigin), NormalizeError> {
    let tisb = ObservationOrigin::Tisb {
        band: Band::Adsb1090,
    };
    let adsr = ObservationOrigin::AdsR {
        band: Band::Adsb1090,
    };

    let (namespace, address, link) = match squitter.address {
        ExtendedSquitterAddress::AdsbIcao { address } => (
            AddressNamespace::Icao,
            address,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
        ExtendedSquitterAddress::AdsbNonIcao { address } => (
            AddressNamespace::AdsbNonIcao,
            address,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
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
            observation.emitter_category =
                emitter_category(identification.type_code, identification.emitter_category);
        }
        ExtendedSquitterMessage::AirbornePosition(position) => {
            observation.position = Some(PositionObservation::CprAirborne(cpr_frame(
                position.odd_cpr,
                position.cpr_latitude,
                position.cpr_longitude,
            )));
            apply_altitude(observation, position.altitude);
            observation.adsb_position_integrity = Some(AdsbPositionIntegrity::airborne(
                position.type_code,
                position.nic_supplement_b,
            )?);
        }
        ExtendedSquitterMessage::SurfacePosition(position) => {
            observation.position = Some(PositionObservation::CprSurface(cpr_frame(
                position.odd_cpr,
                position.cpr_latitude,
                position.cpr_longitude,
            )));
            let mut velocity = VelocityObservation::default();
            velocity.ground_speed_kt = position.ground_speed_knots;
            velocity.track_angle_deg_true = position.ground_track_degrees;
            observation.velocity = Some(velocity);
            observation.adsb_position_integrity =
                Some(AdsbPositionIntegrity::surface(position.type_code)?);
        }
        ExtendedSquitterMessage::AirborneVelocity(velocity) => {
            apply_velocity(observation, velocity);
        }
        ExtendedSquitterMessage::AircraftStatus(status) => {
            observation.emergency = Some(vocabulary::emergency(status.emergency_state));
            observation.squawk = Some(Squawk::try_from(status.squawk)?);
        }
        ExtendedSquitterMessage::OperationalStatus(status) => {
            apply_operational_status(observation, *status)?;
        }
        _ => {}
    }
    Ok(())
}

fn apply_operational_status(
    observation: &mut TrafficObservation,
    status: OperationalStatus,
) -> Result<(), NormalizeError> {
    let (quality, nic_supplement_c) = match status {
        OperationalStatus::Airborne(status) => (status.quality, None),
        OperationalStatus::Surface(status) => {
            (status.quality, status.capability_class.nic_supplement_c)
        }
    };
    let (version, supplement) = match quality.version {
        1 => (AdsbVersion::Do260A, quality.nic_supplement_legacy),
        2 => (AdsbVersion::Do260B, quality.nic_supplement_a),
        _ => return Ok(()),
    };
    observation.adsb_integrity = Some(AdsbIntegrityDeclaration::new(
        version,
        supplement,
        nic_supplement_c,
        quality.navigation_accuracy_category_position,
        quality.source_integrity_level,
    )?);
    Ok(())
}

/// Convert the 1090 ES category set selected by the type code.
///
/// Type code 4 selects aircraft set A. Type code 3 selects aircraft set B.
/// Type code 2 selects surface and obstacle set C. Type code 1 selects the
/// reserved set D. Zero and reserved category codes supply no category.
fn emitter_category(type_code: u8, category: u8) -> Option<EmitterCategory> {
    use EmitterCategory::{
        ClusterObstacle, Glider, HeavyAircraft, HighPerformanceAircraft, HighVortexAircraft,
        LargeAircraft, LightAircraft, LighterThanAir, LineObstacle, Parachutist, PointObstacle,
        Rotorcraft, SmallAircraft, SpaceVehicle, SurfaceEmergencyVehicle, SurfaceServiceVehicle,
        Ultralight, UncrewedAircraft,
    };

    match (type_code, category) {
        (4, 1) => Some(LightAircraft),
        (4, 2) => Some(SmallAircraft),
        (4, 3) => Some(LargeAircraft),
        (4, 4) => Some(HighVortexAircraft),
        (4, 5) => Some(HeavyAircraft),
        (4, 6) => Some(HighPerformanceAircraft),
        (4, 7) => Some(Rotorcraft),
        (3, 1) => Some(Glider),
        (3, 2) => Some(LighterThanAir),
        (3, 3) => Some(Parachutist),
        (3, 4) => Some(Ultralight),
        (3, 6) => Some(UncrewedAircraft),
        (3, 7) => Some(SpaceVehicle),
        (2, 1) => Some(SurfaceEmergencyVehicle),
        (2, 2) => Some(SurfaceServiceVehicle),
        (2, 3) => Some(PointObstacle),
        (2, 4) => Some(ClusterObstacle),
        (2, 5) => Some(LineObstacle),
        _ => None,
    }
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

    let mut fused = VelocityObservation::default();
    fused.ground_speed_kt = ground_speed_kt;
    fused.track_angle_deg_true = track_angle_deg_true;
    fused.airspeed_kt = velocity.airspeed_knots.map(f64::from);
    fused.airspeed_kind = velocity.airspeed_kind.map(vocabulary::airspeed_kind);
    // Subtypes 1 and 2 report a ground track; 3 and 4 report a magnetic
    // heading. Storing the former as a heading would label a track angle as
    // something a consumer steers by, and the track angle is already derived
    // from the velocity components above.
    let degrees = velocity.heading_degrees.map(f64::from);
    if matches!(velocity.subtype, 3 | 4) {
        fused.heading_deg = degrees;
        fused.heading_reference = Some(HeadingReference::MagneticNorth);
    }
    fused.vertical_rate_fpm = velocity.vertical_rate_fpm.map(i32::from);
    fused.vertical_rate_source = velocity
        .vertical_rate_source
        .map(vocabulary::vertical_rate_source);
    observation.velocity = Some(fused);
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

#[cfg(test)]
mod emitter_category_tests {
    use super::*;

    #[test]
    fn maps_each_1090_category_set_independently() {
        assert_eq!(emitter_category(4, 1), Some(EmitterCategory::LightAircraft));
        assert_eq!(emitter_category(3, 1), Some(EmitterCategory::Glider));
        assert_eq!(
            emitter_category(2, 1),
            Some(EmitterCategory::SurfaceEmergencyVehicle)
        );
        assert_eq!(emitter_category(2, 3), Some(EmitterCategory::PointObstacle));
        assert_eq!(
            emitter_category(2, 4),
            Some(EmitterCategory::ClusterObstacle)
        );
        assert_eq!(emitter_category(2, 5), Some(EmitterCategory::LineObstacle));
        assert_eq!(emitter_category(1, 1), None, "set D is reserved");
        assert_eq!(emitter_category(4, 0), None, "zero means no data");
        assert_eq!(emitter_category(3, 5), None, "set B code 5 is reserved");
        assert_eq!(emitter_category(2, 6), None, "set C code 6 is reserved");
    }
}

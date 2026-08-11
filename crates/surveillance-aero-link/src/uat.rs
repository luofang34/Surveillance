//! Normalizing 978 MHz UAT downlink receptions.
//!
//! On 978 MHz the address qualifier separates cooperative addresses from
//! ground-derived ones, but it does not distinguish an ADS-R rebroadcast from
//! a TIS-B report: both reach a receiver through the same ground service and
//! carry the TIS-B qualifiers. Both therefore normalize to a TIS-B link,
//! which is the conservative reading — it never claims a report came straight
//! from the target when it did not.

use aero_link::{
    AddressQualifier, Altitude, AltitudeKind, ModeStatus, StateVector, TrafficProjection,
    UatMessage, UatReception,
};
use surveillance_core::{
    AddressNamespace, Band, Callsign, EmitterCategory, ObservationOrigin, ObservationTime,
    PositionObservation, Squawk, TrackKey, TrafficObservation, VelocityObservation, Wgs84Position,
};

use crate::NormalizeError;
use crate::source::source_ref;
use crate::vocabulary;

/// Normalize one 978 MHz reception.
pub(crate) fn normalize(
    projection: &TrafficProjection<'_>,
    reception: &UatReception,
) -> Result<TrafficObservation, NormalizeError> {
    let UatMessage::Downlink(downlink) = &reception.message else {
        return Err(NormalizeError::NoTargetIdentity { downlink_format: 0 });
    };

    let (namespace, link) = classify(downlink.address_qualifier)?;
    let mut observation = TrafficObservation::new(
        ObservationTime::local(projection.received_at_micros),
        source_ref(projection.source),
        link,
        TrackKey::new(namespace, downlink.address),
    );

    if let Some(state_vector) = downlink.state_vector.as_ref() {
        apply_state_vector(&mut observation, state_vector);
    }
    if let Some(mode_status) = downlink.mode_status.as_ref() {
        apply_mode_status(&mut observation, mode_status)?;
    }
    apply_altitude(&mut observation, downlink.secondary_altitude);
    Ok(observation)
}

/// Map a UAT address qualifier onto a namespace and a link class.
fn classify(
    qualifier: AddressQualifier,
) -> Result<(AddressNamespace, ObservationOrigin), NormalizeError> {
    let ground = ObservationOrigin::Tisb { band: Band::Uat978 };
    Ok(match qualifier {
        AddressQualifier::AdsbIcao => (
            AddressNamespace::Icao,
            ObservationOrigin::AdsbDirect { band: Band::Uat978 },
        ),
        AddressQualifier::National => (
            AddressNamespace::AdsbNonIcao,
            ObservationOrigin::AdsbDirect { band: Band::Uat978 },
        ),
        AddressQualifier::TisbIcao => (AddressNamespace::Icao, ground),
        AddressQualifier::TisbTrackFile => (AddressNamespace::TisbTrackFile, ground),
        AddressQualifier::Vehicle => (
            AddressNamespace::SurfaceVehicle,
            ObservationOrigin::AdsbDirect { band: Band::Uat978 },
        ),
        AddressQualifier::FixedBeacon => (
            AddressNamespace::FixedBeacon,
            ObservationOrigin::AdsbDirect { band: Band::Uat978 },
        ),
        AddressQualifier::Reserved6 => {
            return Err(NormalizeError::ReservedAddressQualifier { qualifier: 6 });
        }
        AddressQualifier::Reserved7 => {
            return Err(NormalizeError::ReservedAddressQualifier { qualifier: 7 });
        }
    })
}

fn apply_state_vector(observation: &mut TrafficObservation, state_vector: &StateVector) {
    if let Some(position) = state_vector.position {
        observation.position = Some(PositionObservation::Decoded(Wgs84Position {
            latitude_deg: position.latitude,
            longitude_deg: position.longitude,
        }));
    }
    apply_altitude(observation, state_vector.altitude);
    observation.air_ground = Some(vocabulary::air_ground(state_vector.air_ground_state));
    observation.quality.nic = Some(state_vector.navigation_integrity_category);

    let degrees = state_vector.heading_degrees.map(f64::from);
    let heading_reference = state_vector
        .heading_kind
        .and_then(vocabulary::heading_reference);
    let reports_track = matches!(
        state_vector.heading_kind,
        Some(aero_link::HeadingKind::Track)
    );
    let (heading_deg, track_angle_deg_true) = if reports_track {
        (None, degrees)
    } else if heading_reference.is_some() {
        (degrees, None)
    } else {
        (None, None)
    };

    // `VelocityObservation` is non-exhaustive so the schema can grow, which
    // means an adapter fills it field by field rather than by literal.
    let mut velocity = VelocityObservation::default();
    velocity.ground_speed_kt = state_vector.speed_knots.map(f64::from);
    velocity.track_angle_deg_true = track_angle_deg_true;
    velocity.heading_deg = heading_deg;
    velocity.heading_reference = heading_reference;
    velocity.vertical_rate_fpm = state_vector.vertical_rate_fpm.map(i32::from);
    velocity.vertical_rate_source = state_vector
        .vertical_rate_source
        .map(vocabulary::vertical_rate_from_altitude);
    observation.velocity = Some(velocity);
}

fn apply_mode_status(
    observation: &mut TrafficObservation,
    mode_status: &ModeStatus,
) -> Result<(), NormalizeError> {
    observation.emitter_category = emitter_category(mode_status.emitter_category);
    observation.emergency = Some(vocabulary::emergency_code(mode_status.emergency_status));
    observation.quality.nac_p = Some(mode_status.nac_p);
    observation.quality.nac_v = Some(mode_status.nac_v);
    observation.quality.sil = Some(mode_status.source_integrity_level);

    let Some(text) = mode_status.callsign.as_ref() else {
        return Ok(());
    };
    match mode_status.callsign_kind {
        Some(aero_link::CallsignKind::Squawk) => {
            if let Ok(code) = text.parse::<u16>() {
                observation.squawk = Some(Squawk::try_from(code)?);
            }
        }
        kind => {
            let mut callsign = Callsign::new(text.as_str());
            callsign.kind = kind.map(vocabulary::callsign_kind);
            observation.callsign = Some(callsign);
        }
    }
    Ok(())
}

/// Convert the flat UAT emitter-category code.
///
/// Codes 1 through 7, 9 through 12, 14, and 15 identify aircraft. Codes 17
/// and 18 identify surface vehicles. Codes 19 through 21 identify obstacle
/// shapes. Codes 0, 8, 13, 16, and 22 through 39 supply no category.
fn emitter_category(code: u8) -> Option<EmitterCategory> {
    use EmitterCategory::{
        ClusterObstacle, Glider, HeavyAircraft, HighPerformanceAircraft, HighVortexAircraft,
        LargeAircraft, LightAircraft, LighterThanAir, LineObstacle, Parachutist, PointObstacle,
        Rotorcraft, SmallAircraft, SpaceVehicle, SurfaceEmergencyVehicle, SurfaceServiceVehicle,
        Ultralight, UncrewedAircraft,
    };

    match code {
        1 => Some(LightAircraft),
        2 => Some(SmallAircraft),
        3 => Some(LargeAircraft),
        4 => Some(HighVortexAircraft),
        5 => Some(HeavyAircraft),
        6 => Some(HighPerformanceAircraft),
        7 => Some(Rotorcraft),
        9 => Some(Glider),
        10 => Some(LighterThanAir),
        11 => Some(Parachutist),
        12 => Some(Ultralight),
        14 => Some(UncrewedAircraft),
        15 => Some(SpaceVehicle),
        17 => Some(SurfaceEmergencyVehicle),
        18 => Some(SurfaceServiceVehicle),
        19 => Some(PointObstacle),
        20 => Some(ClusterObstacle),
        21 => Some(LineObstacle),
        _ => None,
    }
}

fn apply_altitude(observation: &mut TrafficObservation, altitude: Option<Altitude>) {
    let Some(altitude) = altitude else {
        return;
    };
    match altitude.kind {
        AltitudeKind::Barometric => observation.pressure_altitude_ft = Some(altitude.feet),
        AltitudeKind::Geometric => observation.geometric_altitude_ft = Some(altitude.feet),
    }
}

#[cfg(test)]
mod emitter_category_tests {
    use super::*;

    #[test]
    fn maps_uat_aircraft_vehicle_and_obstacle_codes() {
        assert_eq!(emitter_category(1), Some(EmitterCategory::LightAircraft));
        assert_eq!(emitter_category(9), Some(EmitterCategory::Glider));
        assert_eq!(
            emitter_category(17),
            Some(EmitterCategory::SurfaceEmergencyVehicle)
        );
        assert_eq!(emitter_category(19), Some(EmitterCategory::PointObstacle));
        assert_eq!(emitter_category(20), Some(EmitterCategory::ClusterObstacle));
        assert_eq!(emitter_category(21), Some(EmitterCategory::LineObstacle));
        for reserved in [0, 8, 13, 16, 22, 39] {
            assert_eq!(emitter_category(reserved), None, "for code {reserved}");
        }
    }
}

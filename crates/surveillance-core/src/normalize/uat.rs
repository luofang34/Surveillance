//! Normalizing 978 MHz UAT downlink receptions.
//!
//! On 978 MHz the address qualifier separates cooperative addresses from
//! ground-derived ones, but it does not distinguish an ADS-R rebroadcast from
//! a TIS-B report: both reach a receiver through the same ground service and
//! carry the TIS-B qualifiers. Both therefore normalize to a TIS-B link,
//! which is the conservative reading — it never claims a report came straight
//! from the target when it did not.

use aero_link::{
    AddressQualifier, Altitude, AltitudeKind, Band, EmergencyState, HeadingKind, HeadingReference,
    ModeStatus, StateVector, TrafficProjection, UatMessage, UatReception,
};

use crate::normalize::NormalizeError;
use crate::observation::{
    Callsign, PositionObservation, Squawk, TrafficLink, TrafficObservation, VelocityObservation,
    Wgs84Position,
};
use crate::track::{AddressNamespace, TrackKey};

/// Normalize one 978 MHz reception.
pub(super) fn normalize(
    projection: &TrafficProjection<'_>,
    reception: &UatReception,
) -> Result<TrafficObservation, NormalizeError> {
    let UatMessage::Downlink(downlink) = &reception.message else {
        return Err(NormalizeError::NoTargetIdentity { downlink_format: 0 });
    };

    let (namespace, link) = classify(downlink.address_qualifier)?;
    let mut observation = TrafficObservation::new(
        projection.received_at_micros,
        projection.source,
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
) -> Result<(AddressNamespace, TrafficLink), NormalizeError> {
    let ground = TrafficLink::Tisb { band: Band::Uat978 };
    Ok(match qualifier {
        AddressQualifier::AdsbIcao => (AddressNamespace::Icao, TrafficLink::Uat978),
        AddressQualifier::National => (AddressNamespace::AdsbNonIcao, TrafficLink::Uat978),
        AddressQualifier::TisbIcao => (AddressNamespace::Icao, ground),
        AddressQualifier::TisbTrackFile => (AddressNamespace::TisbTrackFile, ground),
        AddressQualifier::Vehicle => (AddressNamespace::SurfaceVehicle, TrafficLink::Uat978),
        AddressQualifier::FixedBeacon => (AddressNamespace::FixedBeacon, TrafficLink::Uat978),
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
    observation.air_ground = Some(state_vector.air_ground_state);
    observation.quality.nic = Some(state_vector.navigation_integrity_category);

    let degrees = state_vector.heading_degrees.map(f64::from);
    let (heading_deg, track_angle_deg_true, heading_reference) = match state_vector.heading_kind {
        Some(HeadingKind::Track) => (None, degrees, None),
        Some(HeadingKind::Magnetic) => (degrees, None, Some(HeadingReference::MagneticNorth)),
        Some(HeadingKind::True) => (degrees, None, Some(HeadingReference::TrueNorth)),
        None => (None, None, None),
    };
    observation.velocity = Some(VelocityObservation {
        ground_speed_kt: state_vector.speed_knots.map(f64::from),
        track_angle_deg_true,
        heading_deg,
        heading_reference,
        vertical_rate_fpm: state_vector.vertical_rate_fpm.map(i32::from),
        vertical_rate_source: state_vector.vertical_rate_source.map(vertical_rate_source),
        ..VelocityObservation::default()
    });
}

/// UAT reports the vertical-rate source as an altitude kind; the exported
/// schema names it with the 1090 vocabulary so one consumer reads both links.
fn vertical_rate_source(kind: AltitudeKind) -> aero_link::VerticalRateSource {
    match kind {
        AltitudeKind::Barometric => aero_link::VerticalRateSource::Barometric,
        AltitudeKind::Geometric => aero_link::VerticalRateSource::Geometric,
    }
}

fn apply_mode_status(
    observation: &mut TrafficObservation,
    mode_status: &ModeStatus,
) -> Result<(), NormalizeError> {
    observation.emitter_category = Some(mode_status.emitter_category);
    observation.emergency = Some(emergency_state(mode_status.emergency_status));
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
            observation.callsign = Some(Callsign {
                text: text.clone(),
                kind,
            });
        }
    }
    Ok(())
}

/// Map the UAT emergency code onto the shared emergency vocabulary; the code
/// points are assigned identically on both links.
fn emergency_state(code: u8) -> EmergencyState {
    match code {
        0 => EmergencyState::None,
        1 => EmergencyState::General,
        2 => EmergencyState::Medical,
        3 => EmergencyState::MinimumFuel,
        4 => EmergencyState::NoCommunication,
        5 => EmergencyState::UnlawfulInterference,
        6 => EmergencyState::DownedAircraft,
        _ => EmergencyState::Reserved,
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

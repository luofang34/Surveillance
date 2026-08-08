//! Mapping `aero-link`'s reported vocabulary onto Surveillance's.
//!
//! Both vocabularies are foreign to this crate, so these are plain functions
//! rather than `From` implementations. That is a feature of the boundary
//! rather than a limitation of it: every conversion is written down in one
//! place, and a variant added upstream fails to compile here instead of
//! silently arriving as something else.

use surveillance_core::{
    AirGroundState, AirspeedKind, CallsignKind, EmergencyState, HeadingReference,
    VerticalRateSource,
};

/// Air/ground state reported by a target.
pub(crate) fn air_ground(value: aero_link::AirGroundState) -> AirGroundState {
    match value {
        aero_link::AirGroundState::Subsonic => AirGroundState::Subsonic,
        aero_link::AirGroundState::Supersonic => AirGroundState::Supersonic,
        aero_link::AirGroundState::Ground => AirGroundState::Ground,
        aero_link::AirGroundState::Reserved => AirGroundState::Reserved,
    }
}

/// Emergency or priority state reported by a target.
pub(crate) fn emergency(value: aero_link::EmergencyState) -> EmergencyState {
    match value {
        aero_link::EmergencyState::None => EmergencyState::None,
        aero_link::EmergencyState::General => EmergencyState::General,
        aero_link::EmergencyState::Medical => EmergencyState::Medical,
        aero_link::EmergencyState::MinimumFuel => EmergencyState::MinimumFuel,
        aero_link::EmergencyState::NoCommunication => EmergencyState::NoCommunication,
        aero_link::EmergencyState::UnlawfulInterference => EmergencyState::UnlawfulInterference,
        aero_link::EmergencyState::DownedAircraft => EmergencyState::DownedAircraft,
        aero_link::EmergencyState::Reserved => EmergencyState::Reserved,
    }
}

/// The UAT emergency code, whose points are assigned identically to the
/// 1090 vocabulary.
pub(crate) fn emergency_code(code: u8) -> EmergencyState {
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

/// Meaning of a reported identification field.
pub(crate) fn callsign_kind(value: aero_link::CallsignKind) -> CallsignKind {
    match value {
        aero_link::CallsignKind::Callsign => CallsignKind::Callsign,
        aero_link::CallsignKind::Squawk => CallsignKind::Squawk,
    }
}

/// Meaning of a reported airspeed.
pub(crate) fn airspeed_kind(value: aero_link::AirspeedKind) -> AirspeedKind {
    match value {
        aero_link::AirspeedKind::Indicated => AirspeedKind::Indicated,
        aero_link::AirspeedKind::True => AirspeedKind::True,
    }
}

/// Sensor a vertical rate was derived from.
pub(crate) fn vertical_rate_source(value: aero_link::VerticalRateSource) -> VerticalRateSource {
    match value {
        aero_link::VerticalRateSource::Barometric => VerticalRateSource::Barometric,
        aero_link::VerticalRateSource::Geometric => VerticalRateSource::Geometric,
    }
}

/// UAT reports its vertical-rate source as an altitude kind; the canonical
/// model names it with one vocabulary so a consumer reads both links alike.
pub(crate) fn vertical_rate_from_altitude(value: aero_link::AltitudeKind) -> VerticalRateSource {
    match value {
        aero_link::AltitudeKind::Barometric => VerticalRateSource::Barometric,
        aero_link::AltitudeKind::Geometric => VerticalRateSource::Geometric,
    }
}

/// Reference direction of a reported heading.
pub(crate) fn heading_reference(value: aero_link::HeadingKind) -> Option<HeadingReference> {
    match value {
        aero_link::HeadingKind::Track => None,
        aero_link::HeadingKind::Magnetic => Some(HeadingReference::MagneticNorth),
        aero_link::HeadingKind::True => Some(HeadingReference::TrueNorth),
    }
}

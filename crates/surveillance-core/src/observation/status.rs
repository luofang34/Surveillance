//! Target status a source reports about itself.

use serde::{Deserialize, Serialize};

/// Source-neutral emitter category reported by ADS-B equipment.
///
/// Link adapters convert their different wire encodings into these values.
/// An absent value means that the link reported no category or a reserved
/// category.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmitterCategory {
    /// Aircraft with a maximum takeoff mass below 7,031 kg.
    LightAircraft,
    /// Aircraft with a maximum takeoff mass from 7,031 kg to 34,019 kg.
    SmallAircraft,
    /// Aircraft with a maximum takeoff mass from 34,019 kg to 136,078 kg.
    LargeAircraft,
    /// Large aircraft that generates a high wake vortex.
    HighVortexAircraft,
    /// Aircraft with a maximum takeoff mass above 136,078 kg.
    HeavyAircraft,
    /// Aircraft with high acceleration and speed capability.
    HighPerformanceAircraft,
    /// Rotorcraft.
    Rotorcraft,
    /// Glider or sailplane.
    Glider,
    /// Lighter-than-air aircraft.
    LighterThanAir,
    /// Parachutist or skydiver.
    Parachutist,
    /// Ultralight aircraft, hang-glider, or paraglider.
    Ultralight,
    /// Uncrewed aircraft.
    UncrewedAircraft,
    /// Space or trans-atmospheric vehicle.
    SpaceVehicle,
    /// Surface emergency vehicle.
    SurfaceEmergencyVehicle,
    /// Surface service vehicle.
    SurfaceServiceVehicle,
    /// Point obstacle or tethered obstacle.
    PointObstacle,
    /// Cluster obstacle.
    ClusterObstacle,
    /// Line obstacle.
    LineObstacle,
}

impl EmitterCategory {
    /// Whether the category identifies a surface vehicle.
    #[must_use]
    pub const fn is_surface_vehicle(self) -> bool {
        matches!(
            self,
            Self::SurfaceEmergencyVehicle | Self::SurfaceServiceVehicle
        )
    }

    /// Whether the category identifies a fixed obstacle.
    #[must_use]
    pub const fn is_fixed_obstacle(self) -> bool {
        matches!(
            self,
            Self::PointObstacle | Self::ClusterObstacle | Self::LineObstacle
        )
    }
}

/// Air/ground state a target reports.
///
/// The two airborne variants exist because the links that carry this field
/// scale velocity differently for subsonic and supersonic flight; a consumer
/// that only needs airborne-or-not should ask [`Self::is_airborne`] rather
/// than matching the variants.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AirGroundState {
    /// Airborne where the source does not declare a velocity scaling regime.
    Airborne,
    /// Airborne with subsonic velocity scaling.
    Subsonic,
    /// Airborne with supersonic velocity scaling.
    Supersonic,
    /// On the ground.
    Ground,
    /// A reserved encoding, retained rather than guessed at.
    Reserved,
}

impl AirGroundState {
    /// Whether the target reported itself airborne.
    ///
    /// A reserved encoding is not a claim of either state, so it answers
    /// `None` rather than defaulting to airborne.
    #[must_use]
    pub fn is_airborne(self) -> Option<bool> {
        match self {
            Self::Airborne | Self::Subsonic | Self::Supersonic => Some(true),
            Self::Ground => Some(false),
            Self::Reserved => None,
        }
    }
}

/// Emergency or priority state a target reports.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmergencyState {
    /// No emergency condition.
    None,
    /// General emergency.
    General,
    /// Lifeguard or medical emergency.
    Medical,
    /// Minimum-fuel condition.
    MinimumFuel,
    /// Loss of radio communications.
    NoCommunication,
    /// Unlawful interference.
    UnlawfulInterference,
    /// Downed aircraft.
    DownedAircraft,
    /// A reserved encoding, retained rather than guessed at.
    Reserved,
}

impl EmergencyState {
    /// Whether the target is declaring something a display should surface.
    #[must_use]
    pub fn is_active(self) -> bool {
        !matches!(self, Self::None | Self::Reserved)
    }
}

/// Meaning of a reported flight identification.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallsignKind {
    /// A flight-plan callsign.
    Callsign,
    /// A Mode A code rendered in the identification field.
    Squawk,
}

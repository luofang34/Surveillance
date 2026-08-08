//! Target status a source reports about itself.

use serde::{Deserialize, Serialize};

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
            Self::Subsonic | Self::Supersonic => Some(true),
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

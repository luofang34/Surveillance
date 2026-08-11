//! Canonical traffic observations and their provenance.
//!
//! One [`TrafficObservation`] is one normalized traffic measurement, whatever
//! produced it: a local radio, an installed panel, a network provider, a
//! replay, or a simulator. It carries the reported over-the-air identity, the
//! link class it came through, monotonic ingress time, source provenance, and
//! only the measured fields the report actually supplied — absence stays
//! explicit.
//!
//! Every type here belongs to Surveillance. Translating one radio reception
//! into these terms is an adapter's job, so no decoder vocabulary reaches a
//! consumer of the engine.

use alloc::string::String;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cpr::CprFrame;
use crate::freshness::FieldQuality;
use crate::source::SourceRef;
use crate::track::TrackKey;

pub use integrity::{
    AdsbIntegrityDeclaration, AdsbPositionIntegrity, AdsbPositionKind, AdsbVersion,
    InvalidAdsbIntegrityDeclaration, InvalidAdsbPositionIntegrity,
};
pub use origin::ObservationOrigin;
pub use status::{AirGroundState, CallsignKind, EmergencyState, EmitterCategory};
pub use time::{ObservationTime, TimeBasis};
pub use velocity::{AirspeedKind, HeadingReference, VelocityObservation, VerticalRateSource};

mod integrity;
mod origin;
mod status;
mod time;
mod velocity;

#[cfg(test)]
mod tests;

/// One normalized traffic measurement consumed by [`crate::SurveillanceEngine`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TrafficObservation {
    /// When the observation was made and how that was established.
    pub time: ObservationTime,
    /// Runtime-local input that delivered the observation.
    pub source: SourceRef,
    /// What produced the observation; the fusion-precedence axis.
    pub origin: ObservationOrigin,
    /// Over-the-air identity this measurement reports.
    pub key: TrackKey,
    /// Decoded position or undecoded CPR frame.
    pub position: Option<PositionObservation>,
    /// Barometric pressure altitude in feet.
    pub pressure_altitude_ft: Option<i32>,
    /// Geometric (GNSS) altitude in feet.
    pub geometric_altitude_ft: Option<i32>,
    /// Horizontal and vertical movement.
    pub velocity: Option<VelocityObservation>,
    /// Flight identification.
    pub callsign: Option<Callsign>,
    /// Mode A transponder code.
    pub squawk: Option<Squawk>,
    /// Air/ground state reported by the target.
    pub air_ground: Option<AirGroundState>,
    /// Emergency or priority status reported by the target.
    pub emergency: Option<EmergencyState>,
    /// Source-neutral emitter category reported by the target.
    pub emitter_category: Option<EmitterCategory>,
    /// ADS-B status declaration that qualifies position messages.
    pub adsb_integrity: Option<AdsbIntegrityDeclaration>,
    /// ADS-B type-code data used to derive position integrity.
    pub adsb_position_integrity: Option<AdsbPositionIntegrity>,
    /// Integrity and accuracy indicators attached to this measurement.
    pub quality: FieldQuality,
}

impl TrafficObservation {
    /// Create an observation carrying identity and provenance only; measured
    /// fields start absent and are set directly on the returned value.
    #[must_use]
    pub fn new(
        time: ObservationTime,
        source: SourceRef,
        origin: ObservationOrigin,
        key: TrackKey,
    ) -> Self {
        Self {
            time,
            source,
            origin,
            key,
            position: None,
            pressure_altitude_ft: None,
            geometric_altitude_ft: None,
            velocity: None,
            callsign: None,
            squawk: None,
            air_ground: None,
            emergency: None,
            emitter_category: None,
            adsb_integrity: None,
            adsb_position_integrity: None,
            quality: FieldQuality::default(),
        }
    }
}

/// Position content of one observation.
///
/// UAT and coarse TIS-B positions arrive decoded; 1090 ES positions arrive as
/// CPR frames whose global decode requires even/odd history held by the
/// engine, so decoding cannot happen during normalization.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PositionObservation {
    /// Position already decoded by the link.
    Decoded(Wgs84Position),
    /// Undecoded airborne CPR frame; global decode pairs even/odd history.
    CprAirborne(CprFrame),
    /// Undecoded surface CPR frame; decode requires a reference position.
    CprSurface(CprFrame),
}

/// WGS84 geographic position.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Wgs84Position {
    /// Latitude in degrees, positive north.
    pub latitude_deg: f64,
    /// Longitude in degrees, positive east.
    pub longitude_deg: f64,
}

/// Flight identification reported by the target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Callsign {
    /// Decoded identification characters, trailing padding removed upstream.
    pub text: String,
    /// Whether `text` is a flight-plan callsign or a registration, when the
    /// link distinguishes the two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<CallsignKind>,
}

impl Callsign {
    /// Create a callsign of unknown kind.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: None,
        }
    }
}

/// Mode A transponder code: four octal digits as read on the transponder.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(into = "u16", try_from = "u16")]
pub struct Squawk(u16);

impl Squawk {
    /// The code as read on the transponder, e.g. `7500`.
    #[must_use]
    pub const fn code(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for Squawk {
    type Error = InvalidSquawk;

    fn try_from(code: u16) -> Result<Self, Self::Error> {
        let digits = [code / 1000, code / 100 % 10, code / 10 % 10, code % 10];
        if code > 7777 || digits.iter().any(|digit| *digit > 7) {
            return Err(InvalidSquawk { code });
        }
        Ok(Self(code))
    }
}

impl From<Squawk> for u16 {
    fn from(squawk: Squawk) -> Self {
        squawk.0
    }
}

/// A value that is not a Mode A code of four octal digits.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("{code} is not a Mode A code of four octal digits")]
pub struct InvalidSquawk {
    /// The rejected value.
    pub code: u16,
}

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
use crate::source::{Band, SourceRef};
use crate::track::TrackKey;

pub use status::{AirGroundState, CallsignKind, EmergencyState};
pub use velocity::{AirspeedKind, HeadingReference, VelocityObservation, VerticalRateSource};

mod status;
mod velocity;

#[cfg(test)]
mod tests;

/// One normalized traffic measurement consumed by [`crate::SurveillanceEngine`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TrafficObservation {
    /// Monotonic reception time in microseconds; orders, ages, and expires
    /// fields. Never UTC.
    pub received_at_micros: u64,
    /// Runtime-local input that delivered the observation.
    pub source: SourceRef,
    /// Originating link class; the fusion-precedence axis.
    pub link: TrafficLink,
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
    /// Emitter category reported by an identification or mode-status
    /// message, which is what resolves a non-ICAO ADS-B address into a
    /// vehicle, obstacle, or aircraft.
    pub emitter_category: Option<u8>,
    /// Integrity and accuracy indicators attached to this measurement.
    pub quality: FieldQuality,
}

impl TrafficObservation {
    /// Create an observation carrying identity and provenance only; measured
    /// fields start absent and are set directly on the returned value.
    #[must_use]
    pub fn new(
        received_at_micros: u64,
        source: SourceRef,
        link: TrafficLink,
        key: TrackKey,
    ) -> Self {
        Self {
            received_at_micros,
            source,
            link,
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
            quality: FieldQuality::default(),
        }
    }
}

/// Link class that carried an observation.
///
/// Fusion precedence between links and freshness windows both key off this
/// classification, and it survives into every exported field's provenance.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "link", rename_all = "snake_case")]
pub enum TrafficLink {
    /// Direct 1090 ES ADS-B transmission from the target.
    Adsb1090,
    /// Mode S surveillance reply (altitude, identity) from the target.
    ModeS,
    /// Direct UAT ADS-B downlink from the target.
    Uat978,
    /// Ground rebroadcast (ADS-R) of a target transmitting on the other band.
    AdsR {
        /// Band the rebroadcast was received on.
        band: Band,
    },
    /// Ground-derived TIS-B report for the target.
    Tisb {
        /// Band the TIS-B uplink was received on.
        band: Band,
    },
}

impl TrafficLink {
    /// Radio band the observation arrived on.
    #[must_use]
    pub fn band(&self) -> Band {
        match self {
            Self::Adsb1090 | Self::ModeS => Band::Adsb1090,
            Self::Uat978 => Band::Uat978,
            Self::AdsR { band } | Self::Tisb { band } => *band,
        }
    }

    /// Whether the value was relayed by ground infrastructure rather than
    /// received directly from the target.
    #[must_use]
    pub fn is_rebroadcast(&self) -> bool {
        matches!(self, Self::AdsR { .. } | Self::Tisb { .. })
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

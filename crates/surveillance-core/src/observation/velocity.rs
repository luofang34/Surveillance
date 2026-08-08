//! Movement a source reports, in explicit units.

use serde::{Deserialize, Serialize};

/// Horizontal and vertical movement reported by one observation.
///
/// Targets report either a ground-referenced velocity (speed over ground and
/// true track) or an air-referenced one (airspeed and heading). Absent parts
/// stay absent rather than being derived, because deriving one from the other
/// needs a wind estimate the engine does not have.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct VelocityObservation {
    /// Speed over ground in knots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_speed_kt: Option<f64>,
    /// True track angle in degrees clockwise from geographic north.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_angle_deg_true: Option<f64>,
    /// Airspeed in knots when the target reports airspeed instead of ground
    /// velocity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub airspeed_kt: Option<f64>,
    /// Whether `airspeed_kt` is true or indicated airspeed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub airspeed_kind: Option<AirspeedKind>,
    /// Heading in degrees when the target reports heading instead of track.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_deg: Option<f64>,
    /// Reference direction for `heading_deg`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_reference: Option<HeadingReference>,
    /// Vertical rate in feet per minute, positive climbing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_rate_fpm: Option<i32>,
    /// Sensor source for `vertical_rate_fpm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_rate_source: Option<VerticalRateSource>,
}

/// Meaning of a reported airspeed.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AirspeedKind {
    /// Indicated airspeed.
    Indicated,
    /// True airspeed.
    True,
}

/// Reference direction a reported heading is measured from.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadingReference {
    /// Geographic north.
    TrueNorth,
    /// Magnetic north.
    MagneticNorth,
}

/// Sensor a vertical rate was derived from.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerticalRateSource {
    /// Pressure-altitude-derived rate.
    Barometric,
    /// Geometric-altitude-derived rate.
    Geometric,
}

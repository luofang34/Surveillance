//! Position of a track at one instant, between reports.
//!
//! A consumer draws a track more often than reports arrive. Drawing only the
//! reported position makes a target step from one report to the next.
//! [`project_position`] gives the position of a track at one instant: the
//! reported position and altitude when nothing better exists, or the
//! reported values advanced along the reported velocity when projection is
//! allowed. The engine owns this because it already holds the report times,
//! the reported velocity, and the track angle; a client-side copy would be a
//! second opinion about where an aircraft is.
//!
//! Projection is a guess between two facts. The reported values stay
//! available and stay authoritative. A projection is marked
//! [`ProjectionBasis::Extrapolated`] so a consumer can show it differently.
//! The bounds that allow projection travel on the snapshot as
//! [`ProjectionBounds`], so a consumer cannot disagree with the producer
//! about them. Projection stops at the smaller of the projection window and
//! the point at which the track is called coasting, and the projected leg
//! never exceeds the distance limit: a straight projection holds the track
//! angle constant, and a turning target curves away from it by a distance,
//! not by a time.

use serde::{Deserialize, Serialize};

use crate::freshness::FreshnessPolicy;
use crate::geodesy::move_along_great_circle;
use crate::observation::{VelocityObservation, VerticalRateSource, Wgs84Position};
use crate::track::{TrackPhase, TrackSnapshot};

#[cfg(test)]
mod tests;

/// Metres in one nautical mile, for the knots-to-metres conversion.
const METRES_PER_NAUTICAL_MILE: f64 = 1_852.0;

/// Microseconds in one hour, for the knots-to-metres conversion.
const MICROS_PER_HOUR: f64 = 3_600_000_000.0;

/// Microseconds in one minute, for the feet-per-minute conversion.
const MICROS_PER_MINUTE: f64 = 60_000_000.0;

/// How a position for one instant was established.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionBasis {
    /// The fused position as reported; no projection was applied.
    Reported,
    /// The reported position advanced along the reported velocity.
    Extrapolated,
}

/// Bounds that allow position projection, exported with each snapshot.
///
/// The engine stamps the bounds in force when it produces a snapshot, so a
/// consumer on the other side of a process or machine boundary projects with
/// the producer's bounds instead of its own guess. Absence on a decoded
/// snapshot means the producer declared no bounds; [`project_position`] then
/// returns the reported position unchanged.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ProjectionBounds {
    /// Window during which a reported position can be advanced along the
    /// reported velocity.
    pub extrapolate_max_micros: u64,
    /// Distance limit for one projected leg, in metres.
    pub extrapolate_max_distance_m: u64,
    /// Time since the newest accepted observation after which a track
    /// coasts; a coasting or silent track is not projected.
    pub coast_after_micros: u64,
    /// Window during which a fused velocity qualifies a projection.
    pub velocity_fresh_micros: u64,
}

impl ProjectionBounds {
    /// Take the projection-relevant bounds from a freshness policy.
    #[must_use]
    pub const fn from_freshness(freshness: &FreshnessPolicy) -> Self {
        Self {
            extrapolate_max_micros: freshness.extrapolate_max_micros,
            extrapolate_max_distance_m: freshness.extrapolate_max_distance_m,
            coast_after_micros: freshness.coast_after_micros,
            velocity_fresh_micros: freshness.velocity_fresh_micros,
        }
    }
}

/// Position of one track at one instant.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PositionProjection {
    /// Position valid at the queried instant.
    pub position: Wgs84Position,
    /// Barometric pressure altitude in feet at the queried instant, absent
    /// when the track reports none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_altitude_ft: Option<i32>,
    /// How `position` was established.
    pub basis: ProjectionBasis,
}

/// Give the position of `snapshot` at `now_micros`.
///
/// Returns `None` when the track has no position. Returns the reported
/// position and altitude when projection is not allowed or not possible: the
/// snapshot declares no bounds, the track coasts or has been silent past the
/// coast window, the position age exceeds `extrapolate_max_micros`, or the
/// velocity is absent, stale, or has no ground speed and track angle.
/// Otherwise returns the reported position advanced along the reported
/// velocity by the position age, the reported pressure altitude advanced by
/// the reported vertical rate, and [`ProjectionBasis::Extrapolated`].
///
/// The horizontal leg never exceeds `extrapolate_max_distance_m`: a turning
/// target curves away from a straight guess, and the distance limit bounds
/// that error. A vertical rate declared geometric does not move a pressure
/// altitude; the two heights use different references.
///
/// The silence check reads `last_observed_at_micros`, the newest measurement
/// time a snapshot exports. Coasting itself reads the newest reception time,
/// so for an observation mapped from a remote clock projection can stop
/// slightly before the track is called coasting. It never stops later.
///
/// This is a pure function of the snapshot and the caller-supplied
/// monotonic time; it changes no state and reads no clock.
#[must_use]
pub fn project_position(snapshot: &TrackSnapshot, now_micros: u64) -> Option<PositionProjection> {
    let position = snapshot.position.as_ref()?;
    let reported = || PositionProjection {
        position: position.value,
        pressure_altitude_ft: snapshot
            .pressure_altitude_ft
            .as_ref()
            .map(|field| field.value),
        basis: ProjectionBasis::Reported,
    };
    let Some(bounds) = snapshot.projection_bounds else {
        return Some(reported());
    };

    let age_micros = position.age_micros(now_micros);
    let silent_micros = now_micros.saturating_sub(snapshot.last_observed_at_micros);
    if age_micros == 0
        || age_micros > bounds.extrapolate_max_micros
        || snapshot.phase != TrackPhase::Active
        || silent_micros > bounds.coast_after_micros
    {
        return Some(reported());
    }

    let Some(velocity) = snapshot.velocity.as_ref() else {
        return Some(reported());
    };
    if !velocity.is_fresh(now_micros, bounds.velocity_fresh_micros) {
        return Some(reported());
    }
    let (Some(ground_speed_kt), Some(track_angle_deg_true)) = (
        velocity.value.ground_speed_kt,
        velocity.value.track_angle_deg_true,
    ) else {
        return Some(reported());
    };

    #[allow(clippy::cast_precision_loss)]
    let age = age_micros as f64;
    let distance_m = (ground_speed_kt * age * METRES_PER_NAUTICAL_MILE / MICROS_PER_HOUR)
        .min(bounds.extrapolate_max_distance_m as f64);
    Some(PositionProjection {
        position: move_along_great_circle(position.value, track_angle_deg_true, distance_m),
        pressure_altitude_ft: projected_altitude(snapshot, velocity.value, age),
        basis: ProjectionBasis::Extrapolated,
    })
}

/// Advance the reported pressure altitude by the reported vertical rate over
/// `age` microseconds, keeping the reported value when no rate applies.
#[allow(clippy::cast_precision_loss)]
fn projected_altitude(
    snapshot: &TrackSnapshot,
    velocity: VelocityObservation,
    age_micros: f64,
) -> Option<i32> {
    let altitude = snapshot.pressure_altitude_ft.as_ref()?;
    let Some(rate_fpm) = velocity.vertical_rate_fpm else {
        return Some(altitude.value);
    };
    if velocity.vertical_rate_source == Some(VerticalRateSource::Geometric) {
        return Some(altitude.value);
    }
    let feet = rate_fpm as f64 * age_micros / MICROS_PER_MINUTE;
    Some(altitude.value + libm::round(feet) as i32)
}

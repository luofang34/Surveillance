//! Position of a track at one instant, between reports.
//!
//! A consumer draws a track more often than reports arrive. Drawing only the
//! reported position makes a target step from one report to the next.
//! [`project_position`] gives the position of a track at one instant: the
//! reported position when nothing better exists, or the reported position
//! advanced along the reported velocity when projection is allowed. The
//! engine owns this because it already holds the report times, the reported
//! velocity, and the track angle; a client-side copy would be a second
//! opinion about where an aircraft is.
//!
//! Projection is a guess between two facts. The reported position stays
//! available and stays authoritative. A projection is marked
//! [`ProjectionBasis::Extrapolated`] so a consumer can show it differently.
//! Projection stops at the smaller of the projection window and the point at
//! which the track is called coasting, and it needs a fresh velocity with a
//! ground speed and a track angle.

use serde::{Deserialize, Serialize};

use crate::freshness::FreshnessPolicy;
use crate::geodesy::move_along_great_circle;
use crate::observation::Wgs84Position;
use crate::track::{TrackPhase, TrackSnapshot};

#[cfg(test)]
mod tests;

/// Metres in one nautical mile, for the knots-to-metres conversion.
const METRES_PER_NAUTICAL_MILE: f64 = 1_852.0;

/// Microseconds in one hour, for the knots-to-metres conversion.
const MICROS_PER_HOUR: f64 = 3_600_000_000.0;

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

/// Position of one track at one instant.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct PositionProjection {
    /// Position valid at the queried instant.
    pub position: Wgs84Position,
    /// How `position` was established.
    pub basis: ProjectionBasis,
}

/// Give the position of `snapshot` at `now_micros`.
///
/// Returns `None` when the track has no position. Returns the reported
/// position when projection is not allowed or not possible: the track coasts
/// or has been silent past the coast window, the position age exceeds
/// `extrapolate_max_micros`, or the velocity is absent, stale, or has no
/// ground speed and track angle. Otherwise returns the reported position
/// advanced along the reported velocity by the position age.
///
/// The silence check reads `last_observed_at_micros`, the newest measurement
/// time a snapshot exports. Coasting itself reads the newest reception time,
/// so for an observation mapped from a remote clock projection can stop
/// slightly before the track is called coasting. It never stops later.
///
/// Pass the freshness policy of the engine that produced the snapshot. A
/// consumer that receives snapshots over a stream must learn the producer's
/// policy from the stream context; a different policy gives different
/// projection bounds.
///
/// This is a pure function of the snapshot and the caller-supplied
/// monotonic time; it changes no state and reads no clock.
#[must_use]
pub fn project_position(
    snapshot: &TrackSnapshot,
    now_micros: u64,
    freshness: &FreshnessPolicy,
) -> Option<PositionProjection> {
    let position = snapshot.position.as_ref()?;
    let reported = || PositionProjection {
        position: position.value,
        basis: ProjectionBasis::Reported,
    };

    let age_micros = position.age_micros(now_micros);
    let silent_micros = now_micros.saturating_sub(snapshot.last_observed_at_micros);
    if age_micros == 0
        || age_micros > freshness.extrapolate_max_micros
        || snapshot.phase != TrackPhase::Active
        || silent_micros > freshness.coast_after_micros
    {
        return Some(reported());
    }

    let Some(velocity) = snapshot.velocity.as_ref() else {
        return Some(reported());
    };
    if !velocity.is_fresh(now_micros, freshness.velocity_fresh_micros) {
        return Some(reported());
    }
    let (Some(ground_speed_kt), Some(track_angle_deg_true)) = (
        velocity.value.ground_speed_kt,
        velocity.value.track_angle_deg_true,
    ) else {
        return Some(reported());
    };

    #[allow(clippy::cast_precision_loss)]
    let distance_m =
        ground_speed_kt * (age_micros as f64) * METRES_PER_NAUTICAL_MILE / MICROS_PER_HOUR;
    Some(PositionProjection {
        position: move_along_great_circle(position.value, track_angle_deg_true, distance_m),
        basis: ProjectionBasis::Extrapolated,
    })
}

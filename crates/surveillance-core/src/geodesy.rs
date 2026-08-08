//! WGS84 distance used by position validation and kinematic correlation.

use crate::observation::Wgs84Position;

#[cfg(test)]
mod tests;

/// Mean Earth radius in metres (IUGG), adequate for the plausibility and
/// correlation gates that consume it.
pub const EARTH_MEAN_RADIUS_M: f64 = 6_371_008.8;

/// Great-circle distance between two WGS84 positions in metres.
///
/// The haversine form stays accurate for the small separations that
/// plausibility and correlation gates evaluate, where a law-of-cosines form
/// loses precision.
#[must_use]
pub fn great_circle_distance_m(from: Wgs84Position, to: Wgs84Position) -> f64 {
    let from_lat = from.latitude_deg.to_radians();
    let to_lat = to.latitude_deg.to_radians();
    let delta_lat = (to.latitude_deg - from.latitude_deg).to_radians();
    let delta_lon = (to.longitude_deg - from.longitude_deg).to_radians();

    let half_lat = libm::sin(delta_lat / 2.0);
    let half_lon = libm::sin(delta_lon / 2.0);
    let a = half_lat * half_lat + libm::cos(from_lat) * libm::cos(to_lat) * half_lon * half_lon;
    2.0 * EARTH_MEAN_RADIUS_M * libm::asin(libm::sqrt(a.clamp(0.0, 1.0)))
}

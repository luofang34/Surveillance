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

/// Move from `from` along `bearing_deg_true` for `distance_m` metres.
///
/// This is the spherical direct problem, the inverse of
/// [`great_circle_distance_m`]: accurate for the short legs between position
/// reports that position projection evaluates. Longitude wraps into
/// `[-180, 180)`.
#[must_use]
pub fn move_along_great_circle(
    from: Wgs84Position,
    bearing_deg_true: f64,
    distance_m: f64,
) -> Wgs84Position {
    let angular = distance_m / EARTH_MEAN_RADIUS_M;
    let bearing = bearing_deg_true.to_radians();
    let from_lat = from.latitude_deg.to_radians();
    let from_lon = from.longitude_deg.to_radians();

    let sin_from_lat = libm::sin(from_lat);
    let cos_from_lat = libm::cos(from_lat);
    let sin_angular = libm::sin(angular);
    let cos_angular = libm::cos(angular);

    let to_lat = libm::asin(
        (sin_from_lat * cos_angular + cos_from_lat * sin_angular * libm::cos(bearing))
            .clamp(-1.0, 1.0),
    );
    let to_lon = from_lon
        + libm::atan2(
            libm::sin(bearing) * sin_angular * cos_from_lat,
            cos_angular - sin_from_lat * libm::sin(to_lat),
        );

    Wgs84Position {
        latitude_deg: to_lat.to_degrees(),
        longitude_deg: libm::fmod(to_lon.to_degrees() + 540.0, 360.0) - 180.0,
    }
}

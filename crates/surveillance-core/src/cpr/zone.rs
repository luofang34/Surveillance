//! CPR latitude-zone geometry.

use core::f64::consts::PI;

/// Number of geographic latitude zones between the equator and a pole.
pub const NZ: f64 = 15.0;

/// Number of longitude zones at `latitude_deg`.
///
/// This is the `NL` function of the CPR definition. The transcendental form
/// is used rather than a boundary table so the zone count matches the
/// specification exactly at every latitude, including the poles where the
/// zone count collapses to one.
#[must_use]
pub fn longitude_zones(latitude_deg: f64) -> f64 {
    let latitude = libm::fabs(latitude_deg);
    if latitude < 1e-9 {
        return 59.0;
    }
    if libm::fabs(latitude - 87.0) < 1e-9 {
        return 2.0;
    }
    if latitude > 87.0 {
        return 1.0;
    }

    let numerator = 1.0 - libm::cos(PI / (2.0 * NZ));
    let cos_latitude = libm::cos(latitude.to_radians());
    let denominator = cos_latitude * cos_latitude;
    let zones = libm::floor(2.0 * PI / libm::acos(1.0 - numerator / denominator));
    zones.max(1.0)
}

/// Floored modulo, which keeps CPR zone indexes non-negative for southern and
/// western coordinates where a truncating remainder would not.
#[must_use]
pub fn floored_modulo(value: f64, modulus: f64) -> f64 {
    value - modulus * libm::floor(value / modulus)
}

/// Fraction of a zone encoded by a 17-bit CPR coordinate.
#[must_use]
pub fn encoded_fraction(encoded: u32) -> f64 {
    f64::from(encoded) / f64::from(1_u32 << 17)
}

//! Reference-based CPR decoding for airborne and surface frames.
//!
//! Local decoding resolves one frame against a known nearby position instead
//! of an even/odd pair. It is never a fallback for a failed global decode:
//! without a trustworthy reference the ambiguity it resolves is real, and
//! guessing would place a target in the wrong zone. Surface frames only
//! decode this way, because their 90-degree zones repeat four times around
//! the globe.

use crate::cpr::zone::{NZ, encoded_fraction, floored_modulo, longitude_zones};
use crate::cpr::{CprFormat, CprFrame};
use crate::observation::Wgs84Position;

/// Decode one frame against a reference position.
///
/// `degrees_per_cycle` is 360 for airborne frames and 90 for surface frames,
/// which repeat once per quadrant.
pub fn decode(frame: CprFrame, reference: Wgs84Position, degrees_per_cycle: f64) -> Wgs84Position {
    let parity_index = match frame.format {
        CprFormat::Even => 0.0,
        CprFormat::Odd => 1.0,
    };

    let latitude_fraction = encoded_fraction(frame.encoded_latitude);
    let zone_height = degrees_per_cycle / (4.0 * NZ - parity_index);
    let latitude_deg = resolve(reference.latitude_deg, zone_height, latitude_fraction);

    let longitude_fraction = encoded_fraction(frame.encoded_longitude);
    let available_zones = (longitude_zones(latitude_deg) - parity_index).max(1.0);
    let zone_width = degrees_per_cycle / available_zones;
    let longitude_deg = resolve(reference.longitude_deg, zone_width, longitude_fraction);

    Wgs84Position {
        latitude_deg,
        longitude_deg,
    }
}

/// Place `fraction` in the zone nearest the reference coordinate.
fn resolve(reference_deg: f64, zone_size: f64, fraction: f64) -> f64 {
    let zone_index = libm::floor(reference_deg / zone_size)
        + libm::floor(0.5 + floored_modulo(reference_deg, zone_size) / zone_size - fraction);
    zone_size * (zone_index + fraction)
}

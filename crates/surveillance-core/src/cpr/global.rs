//! Global airborne CPR decoding from an even/odd frame pair.

use crate::cpr::zone::{NZ, encoded_fraction, floored_modulo, longitude_zones};
use crate::cpr::{CprDecodeError, CprFrame};
use crate::observation::Wgs84Position;

/// Latitude-zone height for one parity, in degrees.
fn latitude_zone_height(parity_index: f64) -> f64 {
    360.0 / (4.0 * NZ - parity_index)
}

/// Decode an even/odd pair without a reference position.
///
/// `newest_is_even` selects which frame's longitude fraction is used, so the
/// decoded position belongs to the most recent reception rather than to an
/// arbitrary half of the pair.
pub fn decode(
    even: CprFrame,
    odd: CprFrame,
    newest_is_even: bool,
) -> Result<Wgs84Position, CprDecodeError> {
    let even_latitude_fraction = encoded_fraction(even.encoded_latitude);
    let odd_latitude_fraction = encoded_fraction(odd.encoded_latitude);

    let zone_index =
        libm::floor(59.0 * even_latitude_fraction - 60.0 * odd_latitude_fraction + 0.5);

    let even_latitude = fold_latitude(
        latitude_zone_height(0.0) * (floored_modulo(zone_index, 60.0) + even_latitude_fraction),
    );
    let odd_latitude = fold_latitude(
        latitude_zone_height(1.0) * (floored_modulo(zone_index, 59.0) + odd_latitude_fraction),
    );

    let even_zones = longitude_zones(even_latitude);
    let odd_zones = longitude_zones(odd_latitude);
    if even_zones != odd_zones {
        return Err(CprDecodeError::LatitudeZoneMismatch {
            even_zones: even_zones as u16,
            odd_zones: odd_zones as u16,
        });
    }

    let latitude_deg = if newest_is_even {
        even_latitude
    } else {
        odd_latitude
    };
    if !(-90.0..=90.0).contains(&latitude_deg) {
        return Err(CprDecodeError::LatitudeOutOfRange { latitude_deg });
    }

    let longitude_deg = decode_longitude(even, odd, even_zones, newest_is_even);
    Ok(Wgs84Position {
        latitude_deg,
        longitude_deg,
    })
}

fn decode_longitude(even: CprFrame, odd: CprFrame, zones: f64, newest_is_even: bool) -> f64 {
    let even_fraction = encoded_fraction(even.encoded_longitude);
    let odd_fraction = encoded_fraction(odd.encoded_longitude);

    let zone_index = libm::floor(even_fraction * (zones - 1.0) - odd_fraction * zones + 0.5);

    let (parity_index, fraction) = if newest_is_even {
        (0.0, even_fraction)
    } else {
        (1.0, odd_fraction)
    };
    let available_zones = (zones - parity_index).max(1.0);
    let zone_width = 360.0 / available_zones;

    fold_longitude(zone_width * (floored_modulo(zone_index, available_zones) + fraction))
}

/// Map a decoded latitude from the CPR northern convention onto ±90 degrees.
fn fold_latitude(latitude_deg: f64) -> f64 {
    if latitude_deg >= 270.0 {
        latitude_deg - 360.0
    } else {
        latitude_deg
    }
}

/// Map a decoded longitude onto ±180 degrees.
fn fold_longitude(longitude_deg: f64) -> f64 {
    if longitude_deg >= 180.0 {
        longitude_deg - 360.0
    } else {
        longitude_deg
    }
}

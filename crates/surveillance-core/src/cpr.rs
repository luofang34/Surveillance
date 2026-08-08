//! Compact Position Reporting frames, pairing history, and decoding.
//!
//! Global airborne CPR decoding needs an even and an odd frame observed close
//! together, so the frames and their pairing history belong to the engine,
//! never to the stateless normalization step. A decode is accepted only after
//! validating pair age, parity, latitude-zone consistency, and decoded-range
//! plausibility. Local CPR is a separate path that requires a trustworthy
//! reference position, and surface frames are likewise reference-dependent.

use thiserror::Error;

use crate::geodesy::great_circle_distance_m;
use crate::observation::Wgs84Position;

pub mod global;
pub mod local;
pub mod zone;

#[cfg(test)]
mod tests;

/// Degrees spanned by one airborne CPR encoding cycle.
pub const AIRBORNE_DEGREES_PER_CYCLE: f64 = 360.0;

/// Degrees spanned by one surface CPR encoding cycle, which repeats once per
/// quadrant and therefore cannot be decoded without a reference position.
pub const SURFACE_DEGREES_PER_CYCLE: f64 = 90.0;

/// Metres in one nautical mile, used to convert a speed bound into a distance
/// bound.
const METRES_PER_NAUTICAL_MILE: f64 = 1852.0;

/// Encoding parity of one CPR frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CprFormat {
    /// Even encoding.
    Even,
    /// Odd encoding.
    Odd,
}

/// One undecoded CPR position report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CprFrame {
    /// Encoding parity.
    pub format: CprFormat,
    /// 17-bit encoded latitude.
    pub encoded_latitude: u32,
    /// 17-bit encoded longitude.
    pub encoded_longitude: u32,
}

impl CprFrame {
    /// Largest value a 17-bit encoded coordinate can take.
    pub const MAX_ENCODED: u32 = (1 << 17) - 1;

    /// Create a frame from 17-bit encoded coordinates.
    pub fn new(
        format: CprFormat,
        encoded_latitude: u32,
        encoded_longitude: u32,
    ) -> Result<Self, CprFrameError> {
        if encoded_latitude > Self::MAX_ENCODED {
            return Err(CprFrameError::EncodedLatitudeRange {
                value: encoded_latitude,
            });
        }
        if encoded_longitude > Self::MAX_ENCODED {
            return Err(CprFrameError::EncodedLongitudeRange {
                value: encoded_longitude,
            });
        }
        Ok(Self {
            format,
            encoded_latitude,
            encoded_longitude,
        })
    }
}

/// A value that cannot form a CPR frame.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CprFrameError {
    /// Encoded latitude exceeds 17 bits.
    #[error("encoded latitude {value} exceeds 17 bits")]
    EncodedLatitudeRange {
        /// The rejected value.
        value: u32,
    },
    /// Encoded longitude exceeds 17 bits.
    #[error("encoded longitude {value} exceeds 17 bits")]
    EncodedLongitudeRange {
        /// The rejected value.
        value: u32,
    },
}

/// Limits every CPR decode is validated against.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct CprPolicy {
    /// Largest separation between the even and odd frames of a global pair.
    /// A wider pair may straddle enough travel to decode into a different
    /// latitude zone than either frame was encoded in.
    pub max_pair_age_micros: u64,
    /// Speed bound converting elapsed time into a plausible travel distance.
    pub max_ground_speed_kt: f64,
    /// Distance allowance added to the travel bound, covering position
    /// uncertainty and the interval before a first velocity is known.
    pub jump_allowance_m: f64,
}

impl Default for CprPolicy {
    fn default() -> Self {
        Self {
            max_pair_age_micros: 10_000_000,
            max_ground_speed_kt: 1_200.0,
            jump_allowance_m: 5_000.0,
        }
    }
}

/// Failure decoding or validating a CPR position.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum CprDecodeError {
    /// No counterpart frame of the opposite parity has been observed yet.
    #[error("no counterpart CPR frame observed yet")]
    PairIncomplete,
    /// The even and odd frames are too far apart in time to pair.
    #[error("CPR pair spans {age_micros} us, limit is {limit_micros} us")]
    PairTooOld {
        /// Separation between the two frames.
        age_micros: u64,
        /// Configured [`CprPolicy::max_pair_age_micros`].
        limit_micros: u64,
    },
    /// The pair halves decoded into different latitude zones, so they do not
    /// describe one position.
    #[error("CPR pair spans latitude zones {even_zones} and {odd_zones}")]
    LatitudeZoneMismatch {
        /// Longitude-zone count of the even half.
        even_zones: u16,
        /// Longitude-zone count of the odd half.
        odd_zones: u16,
    },
    /// The decoded latitude is not a geographic latitude.
    #[error("decoded latitude {latitude_deg} is outside ±90 degrees")]
    LatitudeOutOfRange {
        /// The rejected latitude.
        latitude_deg: f64,
    },
    /// The decode implies travel the target could not have made.
    #[error("decoded position is {distance_m} m from the previous one, limit is {limit_m} m")]
    ImplausibleRange {
        /// Distance from the previously applied position.
        distance_m: f64,
        /// Bound implied by elapsed time and [`CprPolicy::max_ground_speed_kt`].
        limit_m: f64,
    },
    /// A reference-dependent frame arrived with no trustworthy reference.
    #[error("no reference position available for a reference-dependent CPR frame")]
    ReferenceRequired,
}

/// One CPR frame and the time it was received.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TimedCprFrame {
    frame: CprFrame,
    at_micros: u64,
}

/// Even/odd frame history for one track.
///
/// Recording a frame always succeeds; validation happens at decode time so a
/// rejected decode still leaves the newer frame available to pair with the
/// next reception of the opposite parity.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CprHistory {
    even: Option<TimedCprFrame>,
    odd: Option<TimedCprFrame>,
    newest_is_even: bool,
}

impl CprHistory {
    /// Retain `frame` as the newest of its parity.
    pub(crate) fn record(&mut self, frame: CprFrame, at_micros: u64) {
        let timed = TimedCprFrame { frame, at_micros };
        match frame.format {
            CprFormat::Even => {
                self.even = Some(timed);
                self.newest_is_even = true;
            }
            CprFormat::Odd => {
                self.odd = Some(timed);
                self.newest_is_even = false;
            }
        }
    }

    /// Decode the retained pair without a reference position.
    pub(crate) fn decode_global(
        &self,
        policy: &CprPolicy,
    ) -> Result<Wgs84Position, CprDecodeError> {
        let (Some(even), Some(odd)) = (self.even, self.odd) else {
            return Err(CprDecodeError::PairIncomplete);
        };

        let age_micros = even.at_micros.abs_diff(odd.at_micros);
        if age_micros > policy.max_pair_age_micros {
            return Err(CprDecodeError::PairTooOld {
                age_micros,
                limit_micros: policy.max_pair_age_micros,
            });
        }

        global::decode(even.frame, odd.frame, self.newest_is_even)
    }
}

/// Reject a decode that implies travel the target could not have made.
///
/// The bound grows with elapsed time so a long gap does not force a false
/// rejection, and the allowance covers position uncertainty when a target has
/// only just been acquired.
pub fn validate_range(
    candidate: Wgs84Position,
    previous: Wgs84Position,
    elapsed_micros: u64,
    policy: &CprPolicy,
) -> Result<(), CprDecodeError> {
    let elapsed_hours = elapsed_micros as f64 / 3_600_000_000.0;
    let travel_m = policy.max_ground_speed_kt * elapsed_hours * METRES_PER_NAUTICAL_MILE;
    let limit_m = travel_m + policy.jump_allowance_m;
    let distance_m = great_circle_distance_m(previous, candidate);
    if distance_m > limit_m {
        return Err(CprDecodeError::ImplausibleRange {
            distance_m,
            limit_m,
        });
    }
    Ok(())
}

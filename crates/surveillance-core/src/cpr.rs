//! Compact Position Reporting frames awaiting decode.
//!
//! Global airborne CPR decoding needs an even and an odd frame observed close
//! together, so the frames and their pairing history belong to the engine,
//! never to the stateless normalization step. A decode is accepted only after
//! validating pair age, parity, latitude-zone consistency, and decoded-range
//! plausibility. Local CPR is a separate path that requires a trustworthy
//! reference position, and surface frames are likewise reference-dependent.

use thiserror::Error;

#[cfg(test)]
mod tests;

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

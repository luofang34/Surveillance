//! Turning one borrowed `aero-link` traffic projection into a
//! [`TrafficObservation`].
//!
//! Normalization is stateless and synchronous: it classifies the reported
//! identity into an address namespace and a link class, converts measured
//! fields into explicit units, and carries provenance through. It decodes no
//! positions from CPR and remembers nothing between receptions — everything
//! that spans receptions belongs to the engine.
//!
//! Classification reads `aero-link`'s typed fields — the UAT address
//! qualifier and the DF 18 control field — never raw message bytes. A
//! reception that names no target, or that carries service-management data
//! rather than a target, produces a typed error instead of a fabricated
//! identity.

use aero_link::{TrafficPayload, TrafficProjection};
use thiserror::Error;

use crate::observation::{InvalidSquawk, TrafficObservation};

mod adsb1090;
mod uat;

#[cfg(test)]
mod tests;

impl TrafficObservation {
    /// Normalize one borrowed traffic projection.
    ///
    /// Non-traffic receptions never reach this function: the projection is
    /// `None` for them, so the caller skips ingestion entirely and engine
    /// state is untouched.
    pub fn normalize(projection: &TrafficProjection<'_>) -> Result<Self, NormalizeError> {
        match projection.payload {
            TrafficPayload::Adsb1090(message) => adsb1090::normalize(projection, message),
            TrafficPayload::Uat978(reception) => uat::normalize(projection, reception),
            _ => Err(NormalizeError::UnsupportedPayload),
        }
    }
}

/// Failure normalizing one traffic reception.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum NormalizeError {
    /// The reception carries no address that names a target.
    #[error("downlink format {downlink_format} carries no target address")]
    NoTargetIdentity {
        /// Five-bit downlink format of the rejected frame.
        downlink_format: u8,
    },
    /// The address was recovered from parity and no independent observation
    /// has confirmed it.
    ///
    /// Accepting an unconfirmed candidate would create a track for an
    /// address no receiver has actually seen announced, so the reception is
    /// refused until confirmation arrives.
    #[error("downlink format {downlink_format} has an unconfirmed parity-recovered address")]
    UnconfirmedAddress {
        /// Five-bit downlink format of the rejected frame.
        downlink_format: u8,
    },
    /// The frame carries TIS-B or ADS-R service-management data rather than a
    /// target report.
    #[error("extended squitter carries service-management data, not a target")]
    ServiceManagement,
    /// The UAT address qualifier is reserved and has no defined namespace.
    #[error("UAT address qualifier {qualifier} is reserved")]
    ReservedAddressQualifier {
        /// Raw qualifier value, rendered as its protocol number.
        qualifier: u8,
    },
    /// A Mode A code in the reception is not four octal digits.
    #[error("reception carries an invalid Mode A code")]
    Squawk(#[from] InvalidSquawk),
    /// The projection carries a band this version does not normalize.
    ///
    /// The payload enum is non-exhaustive so `aero-link` can add bands
    /// compatibly; a consumer built before a new band exists refuses it
    /// rather than misreading it as a supported one.
    #[error("traffic payload names a band this version does not normalize")]
    UnsupportedPayload,
}

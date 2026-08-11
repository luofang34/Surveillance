//! Traffic projection dispatch and normalization errors.

use aero_link::{TrafficPayload, TrafficProjection};
use surveillance_core::{
    InvalidAdsbIntegrityDeclaration, InvalidAdsbPositionIntegrity, InvalidSquawk,
    TrafficObservation,
};
use thiserror::Error;

use crate::{adsb1090, uat};

/// Normalize one borrowed traffic projection.
///
/// Non-traffic receptions never reach this function. The caller skips them
/// because `ReceptionEvent::traffic` returns `None`. Thus, weather has no path
/// into the engine.
pub fn normalize(projection: &TrafficProjection<'_>) -> Result<TrafficObservation, NormalizeError> {
    match projection.payload {
        TrafficPayload::Adsb1090(message) => adsb1090::normalize(projection, message),
        TrafficPayload::Uat978(reception) => uat::normalize(projection, reception),
        _ => Err(NormalizeError::UnsupportedPayload),
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
    /// Accepting an unconfirmed candidate would create a track for an address
    /// no receiver has announced. The adapter refuses it until confirmation
    /// arrives.
    #[error("downlink format {downlink_format} has an unconfirmed parity-recovered address")]
    UnconfirmedAddress {
        /// Five-bit downlink format of the rejected frame.
        downlink_format: u8,
    },
    /// The frame carries service-management data instead of a target report.
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
    /// ADS-B operational-status integrity fields are inconsistent.
    #[error("reception carries an invalid ADS-B integrity declaration")]
    AdsbIntegrity(#[from] InvalidAdsbIntegrityDeclaration),
    /// ADS-B position metadata does not match its position class.
    #[error("reception carries invalid ADS-B position integrity metadata")]
    AdsbPositionIntegrity(#[from] InvalidAdsbPositionIntegrity),
    /// The projection carries a band this version does not normalize.
    ///
    /// The payload enum is non-exhaustive. Thus, `aero-link` can add a band
    /// without changing existing variants. This adapter refuses a new band
    /// instead of reading it as a supported band.
    #[error("traffic payload names a band this version does not normalize")]
    UnsupportedPayload,
}

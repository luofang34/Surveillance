//! Versioned envelope for exported track records.
//!
//! [`crate::TrackDelta`] and [`crate::TrackSnapshot`] cross process and
//! machine boundaries — native apps, web viewers, recorders — so their
//! serialization is part of the core contract. Every exported delta rides a
//! [`TrackRecord`] whose version is validated before any payload
//! interpretation; readers decode [`TrackRecordHeader`] first so an
//! incompatible payload shape fails with a typed version error rather than a
//! generic deserialization error. Growth rules are additive: an optional
//! field with an absent representation is compatible, and everything else
//! bumps the version.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::track::TrackDelta;

#[cfg(test)]
mod tests;

/// Schema version written by this library.
pub const CURRENT_TRACK_SCHEMA_VERSION: u16 = 1;

/// One versioned exported track delta.
///
/// Fields are private: accessors validate the declared version and return
/// [`TrackSchemaError`] for versions this library does not read.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TrackRecord {
    schema_version: u16,
    delta: TrackDelta,
}

impl TrackRecord {
    /// Wrap a delta with the current schema version.
    #[must_use]
    pub fn new(delta: TrackDelta) -> Self {
        Self {
            schema_version: CURRENT_TRACK_SCHEMA_VERSION,
            delta,
        }
    }

    /// Schema version the record declares.
    #[must_use]
    pub fn schema_version(&self) -> u16 {
        self.schema_version
    }

    /// Borrow the delta after validating the declared version.
    pub fn delta(&self) -> Result<&TrackDelta, TrackSchemaError> {
        validate_version(self.schema_version)?;
        Ok(&self.delta)
    }

    /// Take the delta after validating the declared version.
    pub fn into_delta(self) -> Result<TrackDelta, TrackSchemaError> {
        validate_version(self.schema_version)?;
        Ok(self.delta)
    }
}

/// Version header decoded before a record's payload representation is
/// interpreted.
///
/// Unknown fields are ignored during header decoding, so a header decodes
/// from any record version and version validation can happen first.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct TrackRecordHeader {
    schema_version: u16,
}

impl TrackRecordHeader {
    /// Schema version the record declares.
    #[must_use]
    pub fn schema_version(self) -> u16 {
        self.schema_version
    }

    /// Reject versions this library does not read.
    pub fn validate(self) -> Result<(), TrackSchemaError> {
        validate_version(self.schema_version)
    }
}

fn validate_version(found: u16) -> Result<(), TrackSchemaError> {
    if found == CURRENT_TRACK_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(TrackSchemaError::UnsupportedVersion {
            found,
            supported: CURRENT_TRACK_SCHEMA_VERSION,
        })
    }
}

/// Failure interpreting an exported record.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TrackSchemaError {
    /// The record declares a schema this library does not read.
    #[error("unsupported track schema version {found}, supported version is {supported}")]
    UnsupportedVersion {
        /// Version the record declares.
        found: u16,
        /// Version this library reads.
        supported: u16,
    },
}

/// One monotonic-origin-to-UTC correspondence attached per exported stream.
///
/// All exported times are runtime-monotonic microseconds; the engine never
/// reads a wall clock. A service exporting deltas off the runtime attaches
/// exactly one correspondence per stream at that boundary so consumers can
/// place monotonic times in civil time without per-record UTC stamps.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimeCorrespondence {
    /// Monotonic clock value at the correspondence instant.
    pub monotonic_micros: u64,
    /// Microseconds since the Unix epoch (UTC) at the same instant.
    pub utc_unix_micros: i64,
}

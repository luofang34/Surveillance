//! Versioned envelope for exported track records.
//!
//! [`crate::TrackDelta`] and [`crate::TrackSnapshotHandle`] can cross a process
//! or machine boundary. Their serialization is part of the core contract.
//! Each exported delta uses a [`TrackRecord`]. A reader decodes the
//! [`TrackRecordHeader`] first. It validates the version before it decodes the
//! payload. An incompatible payload thus gives a typed version error. An
//! additive optional field is compatible only when absence has a defined
//! meaning and older readers can ignore it. Adding an enum variant requires a
//! version increase when an older reader can receive it. Removing or renaming
//! a field always requires a version increase. A unit, meaning, tag, or field
//! type change also requires a version increase.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::track::TrackDelta;

#[cfg(test)]
mod tests;

/// Schema version written by this library.
pub const CURRENT_TRACK_SCHEMA_VERSION: u16 = 5;

/// Decode a record in two phases with caller-selected format decoders.
///
/// The header decoder runs first. An unsupported or absent schema version
/// prevents `decode_record` from observing a payload that this build may not
/// understand.
pub fn decode_track_record_with<'a, E, H, R>(
    encoded: &'a [u8],
    decode_header: H,
    decode_record: R,
) -> Result<TrackRecord, TrackRecordDecodeError<E>>
where
    H: FnOnce(&'a [u8]) -> Result<TrackRecordHeader, E>,
    R: FnOnce(&'a [u8]) -> Result<TrackRecord, E>,
{
    let header =
        decode_header(encoded).map_err(|source| TrackRecordDecodeError::Header { source })?;
    header.validate()?;
    let record =
        decode_record(encoded).map_err(|source| TrackRecordDecodeError::Record { source })?;
    record.validate_access_version()?;
    Ok(record)
}

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
        self.validate_access_version()?;
        Ok(&self.delta)
    }

    /// Take the delta after validating the declared version.
    pub fn into_delta(self) -> Result<TrackDelta, TrackSchemaError> {
        self.validate_access_version()?;
        Ok(self.delta)
    }

    fn validate_access_version(&self) -> Result<(), TrackSchemaError> {
        validate_version(self.schema_version)
    }
}

/// Version header decoded before a record's payload representation is
/// interpreted.
///
/// Unknown fields are ignored during header decoding, so a header decodes
/// from any record version and version validation can happen first.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub struct TrackRecordHeader {
    #[serde(default)]
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
    validate_supported_version(found, CURRENT_TRACK_SCHEMA_VERSION)
}

fn validate_supported_version(found: u16, supported: u16) -> Result<(), TrackSchemaError> {
    if found == supported {
        Ok(())
    } else {
        Err(TrackSchemaError::UnsupportedVersion { found, supported })
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

/// Failure loading a versioned track record.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum TrackRecordDecodeError<E> {
    /// The selected format decoder could not inspect the record header.
    #[error("could not decode track record header")]
    Header {
        /// Format-specific decoding failure.
        #[source]
        source: E,
    },
    /// The record uses a schema this library does not read.
    #[error(transparent)]
    Schema(#[from] TrackSchemaError),
    /// The supported record payload is malformed.
    #[error("could not decode track record payload")]
    Record {
        /// Format-specific decoding failure.
        #[source]
        source: E,
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

impl TimeCorrespondence {
    /// Convert a runtime monotonic time to microseconds since the Unix epoch.
    ///
    /// # Errors
    ///
    /// Returns an overflow error when the signed UTC result cannot contain
    /// the monotonic offset.
    pub fn utc_unix_micros_at(self, monotonic_micros: u64) -> Result<i64, TimeCorrespondenceError> {
        let offset = i128::from(monotonic_micros) - i128::from(self.monotonic_micros);
        let utc = i128::from(self.utc_unix_micros) + offset;
        i64::try_from(utc).map_err(|_| TimeCorrespondenceError::UtcRange {
            monotonic_micros,
            correspondence: self,
        })
    }
}

/// One stream-level context for serialized track records.
///
/// A stream writer emits this value once before its records. Individual
/// records stay in runtime-monotonic time and do not repeat UTC data.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TrackStreamHeader {
    schema_version: u16,
    time_correspondence: TimeCorrespondence,
}

impl TrackStreamHeader {
    /// Create a header for records written by this library.
    #[must_use]
    pub const fn new(time_correspondence: TimeCorrespondence) -> Self {
        Self {
            schema_version: CURRENT_TRACK_SCHEMA_VERSION,
            time_correspondence,
        }
    }

    /// Validate and return the stream's one clock correspondence.
    pub fn time_correspondence(self) -> Result<TimeCorrespondence, TrackSchemaError> {
        validate_version(self.schema_version)?;
        Ok(self.time_correspondence)
    }
}

/// Failure converting runtime-monotonic time through a stream correspondence.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TimeCorrespondenceError {
    /// The converted UTC value is outside the signed 64-bit range.
    #[error("UTC conversion for monotonic time {monotonic_micros} is outside the i64 range")]
    UtcRange {
        /// Monotonic time that could not be converted.
        monotonic_micros: u64,
        /// Stream correspondence used for the conversion.
        correspondence: TimeCorrespondence,
    },
}

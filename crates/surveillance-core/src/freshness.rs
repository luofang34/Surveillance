//! Per-field timing, quality, provenance, and freshness policy.
//!
//! Every fused field ages independently: a callsign update cannot refresh an
//! old position. [`TimedField`] binds a value to the observation that
//! supplied it — when it was measured, how that was established, how good it
//! claimed to be, and what produced and delivered it — so a consumer can
//! always explain why one source won. [`FreshnessPolicy`] holds the per-field
//! windows the engine evaluates deterministically inside `advance_time`.

use serde::{Deserialize, Serialize};

use crate::observation::{ObservationOrigin, ObservationTime};
use crate::source::SourceRef;
use crate::track::AddressNamespace;

#[cfg(test)]
mod tests;

/// A fused field value with its observation time, quality, and provenance.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct TimedField<T> {
    /// Fused value.
    pub value: T,
    /// When the supplying observation was made, and how that was established.
    pub time: ObservationTime,
    /// Integrity and accuracy indicators the supplying observation declared.
    pub quality: FieldQuality,
    /// What produced the value and how it reached this runtime.
    pub provenance: FieldProvenance,
}

impl<T> TimedField<T> {
    /// Bind a value to the observation that supplied it.
    #[must_use]
    pub fn new(
        value: T,
        time: ObservationTime,
        quality: FieldQuality,
        provenance: FieldProvenance,
    ) -> Self {
        Self {
            value,
            time,
            quality,
            provenance,
        }
    }

    /// Age of the value at `now_micros`, assuming the pessimistic end of any
    /// timing uncertainty.
    #[must_use]
    pub fn age_micros(&self, now_micros: u64) -> u64 {
        self.time.age_micros(now_micros)
    }

    /// Whether the value is within `window_micros` of `now_micros`.
    #[must_use]
    pub fn is_fresh(&self, now_micros: u64, window_micros: u64) -> bool {
        self.time.is_fresh(now_micros, window_micros)
    }

    /// Monotonic time the value was observed.
    #[must_use]
    pub fn observed_at_micros(&self) -> u64 {
        self.time.observed_at_micros
    }
}

/// Integrity and accuracy indicators, absent when the source does not supply
/// them.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FieldQuality {
    /// Navigation integrity category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nic: Option<u8>,
    /// Navigation accuracy category for position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nac_p: Option<u8>,
    /// Navigation accuracy category for velocity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nac_v: Option<u8>,
    /// Source integrity level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sil: Option<u8>,
}

impl FieldQuality {
    /// Record the indicators a source supplied; absent ones stay absent.
    #[must_use]
    pub fn new(nic: Option<u8>, nac_p: Option<u8>, nac_v: Option<u8>, sil: Option<u8>) -> Self {
        Self {
            nic,
            nac_p,
            nac_v,
            sil,
        }
    }
}

/// Origin of one fused field value.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub struct FieldProvenance {
    /// What produced the supplying observation.
    pub origin: ObservationOrigin,
    /// Address namespace of the key the observation reported.
    pub namespace: AddressNamespace,
    /// Input and incarnation that delivered the observation.
    pub source: SourceRef,
}

impl FieldProvenance {
    /// Record where a fused value came from.
    #[must_use]
    pub fn new(origin: ObservationOrigin, namespace: AddressNamespace, source: SourceRef) -> Self {
        Self {
            origin,
            namespace,
            source,
        }
    }
}

/// Per-field freshness windows and track retention, in monotonic
/// microseconds.
///
/// The defaults allow the documented 13-second upper update interval for
/// TIS-B reports. They remove a silent track after 60 seconds. Applications
/// can use stricter values through [`crate::EngineConfig`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct FreshnessPolicy {
    /// Window during which a fused position is current.
    pub position_fresh_micros: u64,
    /// Window during which a fused velocity is current.
    pub velocity_fresh_micros: u64,
    /// Window during which identity fields (callsign, squawk) are current.
    pub identity_fresh_micros: u64,
    /// Window during which an ADS-B integrity declaration can qualify a
    /// position message.
    pub integrity_fresh_micros: u64,
    /// Age of the newest applied observation after which a track coasts.
    pub coast_after_micros: u64,
    /// Age of the newest applied observation after which a track is removed.
    pub remove_after_micros: u64,
}

impl Default for FreshnessPolicy {
    fn default() -> Self {
        Self {
            position_fresh_micros: 15_000_000,
            velocity_fresh_micros: 15_000_000,
            identity_fresh_micros: 60_000_000,
            integrity_fresh_micros: 5_000_000,
            coast_after_micros: 15_000_000,
            remove_after_micros: 60_000_000,
        }
    }
}

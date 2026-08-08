//! Per-field timing, quality, provenance, and freshness policy.
//!
//! Every fused field ages independently: a callsign update cannot refresh an
//! old position. [`TimedField`] binds a value to the observation that
//! supplied it; [`FreshnessPolicy`] holds the per-field windows the engine
//! evaluates deterministically inside `advance_time`.

use aero_link::ReceiverSource;
use serde::{Deserialize, Serialize};

use crate::observation::TrafficLink;
use crate::track::AddressNamespace;

#[cfg(test)]
mod tests;

/// A fused field value with its observation time, quality, and provenance.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct TimedField<T> {
    /// Fused value.
    pub value: T,
    /// Monotonic time of the observation that supplied the value.
    pub observed_at_micros: u64,
    /// Integrity and accuracy indicators attached to that observation.
    pub quality: FieldQuality,
    /// Link, namespace, and receiver that supplied the value.
    pub provenance: FieldProvenance,
}

impl<T> TimedField<T> {
    /// Bind a value to the observation that supplied it.
    #[must_use]
    pub fn new(
        value: T,
        observed_at_micros: u64,
        quality: FieldQuality,
        provenance: FieldProvenance,
    ) -> Self {
        Self {
            value,
            observed_at_micros,
            quality,
            provenance,
        }
    }

    /// Age of the value at `now_micros` on the shared monotonic clock.
    #[must_use]
    pub fn age_micros(&self, now_micros: u64) -> u64 {
        now_micros.saturating_sub(self.observed_at_micros)
    }

    /// Whether the value is within `window_micros` of `now_micros`.
    #[must_use]
    pub fn is_fresh(&self, now_micros: u64, window_micros: u64) -> bool {
        self.age_micros(now_micros) <= window_micros
    }
}

/// Integrity and accuracy indicators, absent when the link does not supply
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
    /// Record the indicators a link supplied; absent ones stay absent.
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
    /// Link class that carried the supplying observation.
    pub link: TrafficLink,
    /// Address namespace of the key the observation reported.
    pub namespace: AddressNamespace,
    /// Receiver input that produced the observation.
    pub source: ReceiverSource,
}

impl FieldProvenance {
    /// Record where a fused value came from.
    #[must_use]
    pub fn new(link: TrafficLink, namespace: AddressNamespace, source: ReceiverSource) -> Self {
        Self {
            link,
            namespace,
            source,
        }
    }
}

/// Per-field freshness windows and track retention, in monotonic
/// microseconds.
///
/// Values are conservative starting points; applications override them per
/// deployment through [`crate::EngineConfig`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct FreshnessPolicy {
    /// Window during which a fused position is current.
    pub position_fresh_micros: u64,
    /// Window during which a fused velocity is current.
    pub velocity_fresh_micros: u64,
    /// Window during which identity fields (callsign, squawk) are current.
    pub identity_fresh_micros: u64,
    /// Age of the newest applied observation after which a track coasts.
    pub coast_after_micros: u64,
    /// Age of the newest applied observation after which a track is removed.
    pub remove_after_micros: u64,
}

impl Default for FreshnessPolicy {
    fn default() -> Self {
        Self {
            position_fresh_micros: 10_000_000,
            velocity_fresh_micros: 10_000_000,
            identity_fresh_micros: 60_000_000,
            coast_after_micros: 15_000_000,
            remove_after_micros: 90_000_000,
        }
    }
}

//! Single-writer tracking engine.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use thiserror::Error;

use crate::freshness::FreshnessPolicy;
use crate::observation::TrafficObservation;
use crate::track::{TrackDelta, TrackId, TrackKey, TrackSnapshot, TrackState};

#[cfg(test)]
mod tests;

/// Engine tuning supplied by the application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct EngineConfig {
    /// Per-field freshness windows and track retention.
    pub freshness: FreshnessPolicy,
    /// Upper bound on concurrently maintained tracks, `None` for unbounded.
    /// Bounded configurations serve allocation-conscious targets.
    pub max_tracks: Option<usize>,
}

/// Authoritative mutable tracking state.
///
/// The engine is single-writer, synchronous, and runtime-neutral: one owner
/// feeds it observations through [`SurveillanceEngine::ingest`] and drives
/// time through [`SurveillanceEngine::advance_time`]; deltas reach the caller
/// through callbacks on the same stack. There are no channels, tasks, locks,
/// executors, sockets, or timers inside, and the engine never reads a clock —
/// time only advances when the caller says so, which makes expiration
/// deterministic and replay bit-reproducible.
///
/// The tracking pipeline behind these entry points — normalization, CPR,
/// fusion, association, lifecycle — is specified in `docs/architecture.md`;
/// this skeleton maintains no tracks yet and emits no deltas.
#[derive(Clone, Debug)]
pub struct SurveillanceEngine {
    config: EngineConfig,
    tracks: BTreeMap<TrackId, TrackState>,
    keys: BTreeMap<TrackKey, TrackId>,
    ownship: Vec<TrackKey>,
}

impl SurveillanceEngine {
    /// Create an engine with no tracks.
    #[must_use]
    pub fn new(config: EngineConfig) -> Self {
        Self {
            config,
            tracks: BTreeMap::new(),
            keys: BTreeMap::new(),
            ownship: Vec::new(),
        }
    }

    /// Tuning the engine was created with.
    #[must_use]
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Apply one normalized observation, emitting resulting deltas to
    /// `on_delta` before returning.
    ///
    /// Observations may arrive duplicated, delayed, or out of order; ordering
    /// and conflict rules are part of fusion policy, not caller obligations.
    /// The skeleton body accepts observations and emits no deltas.
    pub fn ingest(
        &mut self,
        _observation: TrafficObservation,
        _on_delta: impl FnMut(&TrackDelta),
    ) -> Result<(), IngestError> {
        Ok(())
    }

    /// Advance the engine clock to `now_micros`, emitting coasting and
    /// removal deltas to `on_delta` before returning.
    ///
    /// Expiration happens here and only here; ingestion order never expires
    /// state. The skeleton body maintains no tracks and emits no deltas.
    pub fn advance_time(&mut self, _now_micros: u64, _on_delta: impl FnMut(&TrackDelta)) {}

    /// Replace the caller-supplied ownship identity set used for
    /// ownship-shadow labeling.
    ///
    /// The engine labels matching tracks as shadows and suppresses nothing;
    /// ownship selection and display policy stay in the application.
    pub fn set_ownship_identities(&mut self, identities: &[TrackKey]) {
        self.ownship.clear();
        self.ownship.extend_from_slice(identities);
    }

    /// Ownship identities the engine labels shadows from.
    #[must_use]
    pub fn ownship_identities(&self) -> &[TrackKey] {
        &self.ownship
    }

    /// Number of maintained tracks.
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Snapshot of the track whose primary key is `key`, when one exists.
    #[must_use]
    pub fn snapshot(&self, key: TrackKey) -> Option<&TrackSnapshot> {
        let id = self.keys.get(&key)?;
        self.tracks.get(id).map(|state| &state.snapshot)
    }

    /// Iterate snapshots of every maintained track in [`TrackId`] order.
    pub fn snapshots(&self) -> impl Iterator<Item = &TrackSnapshot> {
        self.tracks.values().map(|state| &state.snapshot)
    }
}

/// Failure applying one observation.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum IngestError {
    /// The configured track bound is exhausted and the observation names a
    /// new target.
    #[error("track capacity {capacity} exhausted")]
    TrackCapacityExceeded {
        /// Configured [`EngineConfig::max_tracks`] bound.
        capacity: usize,
    },
}

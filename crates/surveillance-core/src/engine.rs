//! Single-writer tracking engine.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use thiserror::Error;

use crate::association::{self, CorrelationPolicy};
use crate::cpr::CprPolicy;
use crate::freshness::FreshnessPolicy;
use crate::fusion::FieldUpdate;
use crate::observation::{TrafficObservation, Wgs84Position};
use crate::track::{TrackDelta, TrackId, TrackKey, TrackPhase, TrackSnapshot, TrackState};

use position::PositionContext;

mod ingest;
mod lifecycle;
mod merge;
mod position;

#[cfg(test)]
mod tests;

/// Engine tuning supplied by the application.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct EngineConfig {
    /// Per-field freshness windows and track retention.
    pub freshness: FreshnessPolicy,
    /// Upper bound on concurrently maintained tracks, `None` for unbounded.
    /// Bounded configurations serve allocation-conscious targets.
    pub max_tracks: Option<usize>,
    /// CPR pairing and position-plausibility limits.
    pub cpr: CprPolicy,
    /// Kinematic correlation thresholds and hysteresis.
    pub correlation: CorrelationPolicy,
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
/// A delta materializes one snapshot clone, so deltas are emitted only when a
/// consumer-visible value changes; duplicate receptions that merely refresh
/// freshness and provenance stay silent.
#[derive(Clone, Debug)]
pub struct SurveillanceEngine {
    config: EngineConfig,
    tracks: BTreeMap<TrackId, TrackState>,
    keys: BTreeMap<TrackKey, TrackId>,
    ownship: Vec<TrackKey>,
    reference: Option<Wgs84Position>,
    next_id: u64,
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
            reference: None,
            next_id: 0,
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
    /// and conflict rules are fusion policy, not caller obligations.
    pub fn ingest(
        &mut self,
        observation: TrafficObservation,
        mut on_delta: impl FnMut(&TrackDelta),
    ) -> Result<(), IngestError> {
        let (id, created) = self.resolve_track(&observation)?;
        let policy = self.config.freshness;
        let context = PositionContext {
            freshness: &policy,
            cpr: &self.config.cpr,
            reference: self.reference,
        };
        let Some(state) = self.tracks.get_mut(&id) else {
            return Ok(());
        };

        let mut changed = ingest::apply_fields(&mut state.snapshot, &observation, &policy);
        let position_update = position::apply(state, &observation, context);
        changed |= position_update.is_meaningful();

        state.mark_identity_seen(observation.key, observation.received_at_micros);
        if observation.received_at_micros > state.snapshot.last_observed_at_micros {
            state.snapshot.last_observed_at_micros = observation.received_at_micros;
        }
        if state.snapshot.phase == TrackPhase::Coasting {
            state.snapshot.phase = TrackPhase::Active;
            changed = true;
        }

        if created {
            on_delta(&TrackDelta::Created(state.snapshot.clone()));
        } else if changed {
            on_delta(&TrackDelta::Updated(state.snapshot.clone()));
        }

        if position_update != FieldUpdate::Rejected {
            self.correlate(id, observation.received_at_micros, &mut on_delta);
        }
        Ok(())
    }

    /// Compare one track against every peer it may correlate with, fold the
    /// outcome into its evidence, and merge when the evidence is sufficient.
    ///
    /// This is linear in the track count and runs only when a position
    /// actually changed, so a stream of identity-only receptions costs
    /// nothing.
    fn correlate(
        &mut self,
        subject: TrackId,
        now_micros: u64,
        on_delta: &mut impl FnMut(&TrackDelta),
    ) {
        let outcomes = self.compare_peers(subject, now_micros);
        if outcomes.is_empty() {
            return;
        }

        let policy = self.config.correlation;
        let Some(state) = self.tracks.get_mut(&subject) else {
            return;
        };
        let Some(peer) = association::record(&mut state.correlation, &outcomes, &policy) else {
            return;
        };

        let (Some(subject_state), Some(peer_state)) =
            (self.tracks.get(&subject), self.tracks.get(&peer))
        else {
            return;
        };
        let survivor = merge::survivor_of(subject_state, peer_state);
        let absorbed = if survivor == subject { peer } else { subject };
        merge::merge(
            &mut self.tracks,
            &mut self.keys,
            survivor,
            absorbed,
            &self.config.freshness,
            on_delta,
        );
    }

    fn compare_peers(&self, subject: TrackId, now_micros: u64) -> Vec<(TrackId, bool)> {
        let Some(state) = self.tracks.get(&subject) else {
            return Vec::new();
        };
        self.tracks
            .iter()
            .filter(|(id, _)| **id != subject)
            .filter(|(_, peer)| {
                association::may_correlate(
                    state.snapshot.key.namespace,
                    peer.snapshot.key.namespace,
                )
            })
            .filter_map(|(id, peer)| {
                association::agrees(
                    &state.snapshot,
                    &peer.snapshot,
                    now_micros,
                    &self.config.correlation,
                    &self.config.freshness,
                )
                .map(|agrees| (*id, agrees))
            })
            .collect()
    }

    /// Advance the engine clock to `now_micros`, emitting coasting and
    /// removal deltas to `on_delta` before returning.
    ///
    /// Expiration happens here and only here; ingestion order never expires
    /// state.
    pub fn advance_time(&mut self, now_micros: u64, mut on_delta: impl FnMut(&TrackDelta)) {
        lifecycle::advance(
            &mut self.tracks,
            &mut self.keys,
            now_micros,
            &self.config,
            &mut on_delta,
        );
    }

    /// Replace the caller-supplied ownship identity set used for
    /// ownship-shadow labeling, emitting an update for every track whose
    /// label changes.
    ///
    /// The engine labels matching tracks as shadows and suppresses nothing;
    /// ownship selection and display policy stay in the application.
    pub fn set_ownship_identities(
        &mut self,
        identities: &[TrackKey],
        mut on_delta: impl FnMut(&TrackDelta),
    ) {
        self.ownship.clear();
        self.ownship.extend_from_slice(identities);

        for state in self.tracks.values_mut() {
            let shadow = state
                .snapshot
                .identities
                .associated()
                .iter()
                .chain(core::iter::once(&state.snapshot.identities.primary()))
                .any(|key| self.ownship.contains(key));
            if state.snapshot.ownship_shadow != shadow {
                state.snapshot.ownship_shadow = shadow;
                on_delta(&TrackDelta::Updated(state.snapshot.clone()));
            }
        }
    }

    /// Ownship identities the engine labels shadows from.
    #[must_use]
    pub fn ownship_identities(&self) -> &[TrackKey] {
        &self.ownship
    }

    /// Supply the reference position used to resolve reference-dependent CPR
    /// frames — surface frames, and airborne frames for a track with no
    /// position of its own.
    ///
    /// A track's own fresh position always takes precedence. Passing `None`
    /// withdraws the reference, after which such frames simply do not decode.
    pub fn set_reference_position(&mut self, reference: Option<Wgs84Position>) {
        self.reference = reference;
    }

    /// Reference position reference-dependent frames resolve against.
    #[must_use]
    pub fn reference_position(&self) -> Option<Wgs84Position> {
        self.reference
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

    /// Snapshot of the track carrying `id`, when one exists.
    #[must_use]
    pub fn snapshot_by_id(&self, id: TrackId) -> Option<&TrackSnapshot> {
        self.tracks.get(&id).map(|state| &state.snapshot)
    }

    /// Iterate snapshots of every maintained track in [`TrackId`] order.
    pub fn snapshots(&self) -> impl Iterator<Item = &TrackSnapshot> {
        self.tracks.values().map(|state| &state.snapshot)
    }

    /// Find the existing track for an observation, or create one.
    fn resolve_track(
        &mut self,
        observation: &TrafficObservation,
    ) -> Result<(TrackId, bool), IngestError> {
        if let Some(id) = self.keys.get(&observation.key) {
            return Ok((*id, false));
        }
        if let Some(capacity) = self.config.max_tracks
            && self.tracks.len() >= capacity
        {
            return Err(IngestError::TrackCapacityExceeded { capacity });
        }

        let id = TrackId::new(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        let mut snapshot = TrackSnapshot::new(id, observation.key, observation.received_at_micros);
        snapshot.ownship_shadow = self.ownship.contains(&observation.key);
        self.tracks.insert(id, TrackState::from(snapshot));
        self.keys.insert(observation.key, id);
        Ok((id, true))
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

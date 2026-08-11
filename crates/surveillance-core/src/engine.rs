//! Single-writer tracking engine.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use thiserror::Error;

use crate::association::{self, CorrelationPolicy};
use crate::cpr::CprPolicy;
use crate::freshness::FreshnessPolicy;
use crate::fusion::FieldUpdate;
use crate::observation::{TrafficObservation, Wgs84Position};
use crate::track::{
    ProducerInstanceId, TrackDelta, TrackId, TrackKey, TrackPhase, TrackSnapshot,
    TrackSnapshotHandle, TrackState,
};

pub use outcome::{
    FieldDisposition, FieldOutcomes, IngestOutcome, PositionDisposition, PositionRejection,
};
use position::PositionContext;
pub use publication::SnapshotPublicationError;
use publication::SnapshotPublisher;

mod classification;
mod ingest;
mod integrity;
mod lifecycle;
mod merge;
mod outcome;
mod position;
mod publication;
mod resolution;

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
/// A delta materializes one snapshot clone. Deltas report value, quality, and
/// provenance changes. Repeated receptions from one source that only refresh
/// time stay silent.
#[derive(Debug)]
pub struct SurveillanceEngine {
    config: EngineConfig,
    producer_instance_id: ProducerInstanceId,
    tracks: BTreeMap<TrackId, TrackState>,
    keys: BTreeMap<TrackKey, TrackId>,
    ownship: Vec<TrackKey>,
    reference: Option<Wgs84Position>,
    next_id: u64,
}

impl SurveillanceEngine {
    /// Create an engine with no tracks.
    ///
    /// The caller must supply a new `producer_instance_id` for each engine
    /// construction. The core has no clock, random source, or global identity
    /// registry from which it can make this value.
    #[must_use]
    pub fn new(config: EngineConfig, producer_instance_id: ProducerInstanceId) -> Self {
        Self {
            config,
            producer_instance_id,
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

    /// Identity supplied for this engine construction.
    #[must_use]
    pub fn producer_instance_id(&self) -> ProducerInstanceId {
        self.producer_instance_id
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
    ) -> Result<IngestOutcome, IngestError> {
        let classified_key = observation
            .emitter_category
            .and_then(|category| classification::classified_key(observation.key, category));
        let (id, created) = self.resolve_track(&observation, classified_key)?;
        let policy = self.config.freshness;
        let context = PositionContext {
            freshness: &policy,
            cpr: &self.config.cpr,
            reference: self.reference,
        };
        let Some(state) = self.tracks.get_mut(&id) else {
            return Err(IngestError::TrackStateMissing { id });
        };
        let publisher = SnapshotPublisher::new(self.producer_instance_id);
        publisher.ensure_capacity(state, 1)?;

        let integrity_result = integrity::apply(state, &observation, &policy);
        let mut field_outcomes = ingest::apply_fields(&mut state.snapshot, &observation, &policy);
        field_outcomes.adsb_integrity = integrity_result.declaration;
        field_outcomes.adsb_position_integrity = integrity_result.position_code;
        let position_result = position::apply(state, &observation, context);
        let mut snapshot_changed = field_outcomes.was_applied()
            || position_result.was_applied()
            || integrity_result.position_metadata_changed;
        let mut meaningful = field_outcomes.is_meaningful()
            || position_result.is_meaningful()
            || integrity_result.position_metadata_changed;

        let classified_key = classification::apply(state, &observation, &self.ownship);
        if classified_key.is_some() {
            snapshot_changed = true;
            meaningful = true;
        }

        state.mark_identity_seen(observation.key, observation.time.received_at_micros);
        let observed_at = observation.time.observed_at_micros;
        if observed_at > state.snapshot.last_observed_at_micros {
            state.snapshot.last_observed_at_micros = observed_at;
            snapshot_changed = true;
        }
        if state.snapshot.phase == TrackPhase::Coasting {
            state.snapshot.phase = TrackPhase::Active;
            snapshot_changed = true;
            meaningful = true;
        }

        let delta_emitted = created || meaningful;
        if created {
            let delta = TrackDelta::Created(publisher.snapshot(state)?);
            on_delta(&delta);
        } else if meaningful {
            let delta = TrackDelta::Updated(publisher.snapshot(state)?);
            on_delta(&delta);
        } else if snapshot_changed {
            publisher.revise(state)?;
        }

        if let Some(key) = classified_key {
            self.keys.insert(key, id);
        }

        if position_result.update != FieldUpdate::Rejected {
            self.correlate(id, observation.time.received_at_micros, &mut on_delta)?;
        }
        Ok(IngestOutcome::new(
            id,
            created,
            position_result.disposition,
            field_outcomes,
            delta_emitted,
        ))
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
    ) -> Result<(), SnapshotPublicationError> {
        let outcomes = self.compare_peers(subject, now_micros);
        if outcomes.is_empty() {
            return Ok(());
        }

        let policy = self.config.correlation;
        let Some(state) = self.tracks.get_mut(&subject) else {
            return Ok(());
        };
        let Some(peer) =
            association::record(&mut state.correlation, &outcomes, now_micros, &policy)
        else {
            return Ok(());
        };

        let (Some(subject_state), Some(peer_state)) =
            (self.tracks.get(&subject), self.tracks.get(&peer))
        else {
            return Ok(());
        };
        let survivor = merge::survivor_of(subject_state, peer_state);
        let absorbed = if survivor == subject { peer } else { subject };
        merge::merge(
            &mut self.tracks,
            &mut self.keys,
            survivor,
            absorbed,
            &self.config.freshness,
            &SnapshotPublisher::new(self.producer_instance_id),
            on_delta,
        )?;
        Ok(())
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
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotPublicationError`] before a subject's revision
    /// would wrap.
    pub fn advance_time(
        &mut self,
        now_micros: u64,
        mut on_delta: impl FnMut(&TrackDelta),
    ) -> Result<(), SnapshotPublicationError> {
        let publisher = SnapshotPublisher::new(self.producer_instance_id);
        lifecycle::advance(
            &mut self.tracks,
            &mut self.keys,
            now_micros,
            &self.config,
            &publisher,
            &mut on_delta,
        )
    }

    /// Replace the caller-supplied ownship identity set used for
    /// ownship-shadow labeling, emitting an update for every track whose
    /// label changes.
    ///
    /// The engine labels matching tracks as shadows and suppresses nothing;
    /// ownship selection and display policy stay in the application.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotPublicationError`] before a changed subject's
    /// revision would wrap.
    pub fn set_ownship_identities(
        &mut self,
        identities: &[TrackKey],
        mut on_delta: impl FnMut(&TrackDelta),
    ) -> Result<(), SnapshotPublicationError> {
        let publisher = SnapshotPublisher::new(self.producer_instance_id);
        for state in self.tracks.values() {
            let shadow = is_ownship_shadow(&state.snapshot, identities);
            if state.snapshot.ownship_shadow != shadow {
                publisher.ensure_capacity(state, 1)?;
            }
        }

        self.ownship.clear();
        self.ownship.extend_from_slice(identities);

        for state in self.tracks.values_mut() {
            let shadow = is_ownship_shadow(&state.snapshot, &self.ownship);
            if state.snapshot.ownship_shadow != shadow {
                state.snapshot.ownship_shadow = shadow;
                let delta = TrackDelta::Updated(publisher.snapshot(state)?);
                on_delta(&delta);
            }
        }
        Ok(())
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

    /// Retainable snapshot of the track whose primary key is `key`.
    #[must_use]
    pub fn snapshot(&self, key: TrackKey) -> Option<TrackSnapshotHandle> {
        let id = self.keys.get(&key)?;
        self.tracks
            .get(id)
            .map(|state| state.handle(self.producer_instance_id))
    }

    /// Retainable snapshot of the track carrying `id`.
    #[must_use]
    pub fn snapshot_by_id(&self, id: TrackId) -> Option<TrackSnapshotHandle> {
        self.tracks
            .get(&id)
            .map(|state| state.handle(self.producer_instance_id))
    }

    /// Iterate retainable snapshots in [`TrackId`] order.
    pub fn snapshots(&self) -> impl Iterator<Item = TrackSnapshotHandle> + '_ {
        self.tracks
            .values()
            .map(|state| state.handle(self.producer_instance_id))
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
    /// The next wrapped track identity still names a live track.
    #[error("track identity {id:?} is still live; start a new producer instance")]
    TrackIdentityExhausted {
        /// Colliding identity.
        id: TrackId,
    },
    /// A wire identity and its emitter classification already select
    /// different live tracks.
    #[error(
        "wire key {observed:?} selects {observed_id:?}, but classified key {classified:?} selects {classified_id:?}"
    )]
    IdentityClassificationConflict {
        /// Unclassified wire key.
        observed: TrackKey,
        /// Key implied by the emitter category.
        classified: TrackKey,
        /// Track selected by the wire key.
        observed_id: TrackId,
        /// Track selected by the classified key.
        classified_id: TrackId,
    },
    /// The key index selected a track state that is not present.
    #[error("track state {id:?} is missing")]
    TrackStateMissing {
        /// Missing track identity.
        id: TrackId,
    },
    /// The engine cannot publish the resulting track state.
    #[error("publishing the track state failed")]
    SnapshotPublication {
        /// Publication failure with the producer and subject context.
        #[from]
        source: SnapshotPublicationError,
    },
}

fn is_ownship_shadow(snapshot: &TrackSnapshot, ownship: &[TrackKey]) -> bool {
    snapshot
        .identities
        .associated()
        .iter()
        .chain(core::iter::once(&snapshot.identities.primary()))
        .any(|key| ownship.contains(key))
}

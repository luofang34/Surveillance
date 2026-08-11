//! Typed aircraft feature changes.

use surveillance_core::{
    PositionStatus, ProducerInstanceId, RemovalReason, SnapshotRevision, TimedField, TrackDelta,
    TrackId, TrackSnapshot, TrackSnapshotHandle, Wgs84Position,
};

#[cfg(test)]
mod tests;

/// Identity of one feature across engine restarts.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FeatureId {
    producer_instance_id: ProducerInstanceId,
    track_id: TrackId,
}

impl FeatureId {
    /// Create a feature identity from its producer and track identities.
    #[must_use]
    pub const fn new(producer_instance_id: ProducerInstanceId, track_id: TrackId) -> Self {
        Self {
            producer_instance_id,
            track_id,
        }
    }

    /// Identity of the producer that owns this feature.
    #[must_use]
    pub const fn producer_instance_id(self) -> ProducerInstanceId {
        self.producer_instance_id
    }

    /// Track identity within the producer instance.
    #[must_use]
    pub const fn track_id(self) -> TrackId {
        self.track_id
    }
}

/// One renderer-neutral change to an aircraft feature set.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "boxing would add an allocation to each typed upsert"
)]
pub enum FeatureDelta {
    /// Place or move an aircraft feature in WGS84 coordinates.
    Upsert(AircraftFeature),
    /// Mark an existing feature as stale without removing it.
    Stale {
        /// Stable identity of the feature and its producer.
        id: FeatureId,
        /// Revision assigned to this stale state.
        revision: SnapshotRevision,
    },
    /// Remove an existing feature.
    Remove {
        /// Stable identity of the feature and its producer.
        id: FeatureId,
        /// Revision assigned to this removal.
        revision: SnapshotRevision,
        /// Track that absorbed this feature, if a merge caused removal.
        transfer_to: Option<FeatureId>,
    },
}

/// One aircraft feature with its complete published track state.
///
/// The position retains its observation time, quality, and provenance. The
/// snapshot retains all other exported properties, including
/// `ownship_shadow`. The handle also identifies the producer and revision.
#[derive(Clone, Debug, PartialEq)]
pub struct AircraftFeature {
    position: TimedField<Wgs84Position>,
    snapshot: TrackSnapshotHandle,
}

impl AircraftFeature {
    fn new(position: TimedField<Wgs84Position>, snapshot: TrackSnapshotHandle) -> Self {
        Self { position, snapshot }
    }

    /// Track identity within the producer instance.
    #[must_use]
    pub fn id(&self) -> TrackId {
        self.snapshot.id
    }

    /// Stable identity of this feature and its producer.
    #[must_use]
    pub fn feature_id(&self) -> FeatureId {
        FeatureId::new(self.producer_instance_id(), self.id())
    }

    /// WGS84 position with observation time, quality, and provenance.
    #[must_use]
    pub const fn position(&self) -> &TimedField<Wgs84Position> {
        &self.position
    }

    /// Whether the engine labeled this track as an ownship shadow.
    #[must_use]
    pub fn ownship_shadow(&self) -> bool {
        self.snapshot.ownship_shadow
    }

    /// Identity of the engine that published this state.
    #[must_use]
    pub const fn producer_instance_id(&self) -> ProducerInstanceId {
        self.snapshot.producer_instance_id()
    }

    /// Revision assigned when this state was published.
    #[must_use]
    pub const fn snapshot_revision(&self) -> SnapshotRevision {
        self.snapshot.snapshot_revision()
    }

    /// Complete immutable track state.
    #[must_use]
    pub const fn snapshot(&self) -> &TrackSnapshot {
        self.snapshot.snapshot()
    }

    /// Retainable envelope for the complete published track state.
    #[must_use]
    pub const fn snapshot_handle(&self) -> &TrackSnapshotHandle {
        &self.snapshot
    }

    /// Take the retainable snapshot handle from this feature.
    #[must_use]
    pub fn into_snapshot_handle(self) -> TrackSnapshotHandle {
        self.snapshot
    }
}

/// Map one track delta into a renderer-neutral feature change.
///
/// For a snapshot delta, the function clones the retainable handle only when
/// it produces an upsert.
#[must_use]
pub fn map_track_delta(delta: &TrackDelta) -> Option<FeatureDelta> {
    match delta {
        TrackDelta::Created(snapshot) => {
            if snapshot.position_status == PositionStatus::Current {
                map_upsert(snapshot)
            } else {
                None
            }
        }
        TrackDelta::Updated(snapshot) | TrackDelta::Coasting(snapshot) => {
            match snapshot.position_status {
                PositionStatus::Current => map_upsert(snapshot),
                PositionStatus::Stale => Some(stale(snapshot)),
                PositionStatus::Unavailable => None,
                _ => None,
            }
        }
        TrackDelta::Removed {
            producer_instance_id,
            snapshot_revision,
            id,
            reason,
            ..
        } => Some(FeatureDelta::Remove {
            id: FeatureId::new(*producer_instance_id, *id),
            revision: *snapshot_revision,
            transfer_to: transfer_target(*producer_instance_id, *reason),
        }),
        _ => None,
    }
}

fn stale(snapshot: &TrackSnapshotHandle) -> FeatureDelta {
    FeatureDelta::Stale {
        id: FeatureId::new(snapshot.producer_instance_id(), snapshot.id),
        revision: snapshot.snapshot_revision(),
    }
}

fn map_upsert(snapshot: &TrackSnapshotHandle) -> Option<FeatureDelta> {
    snapshot
        .position
        .map(|position| FeatureDelta::Upsert(AircraftFeature::new(position, snapshot.clone())))
}

fn transfer_target(
    producer_instance_id: ProducerInstanceId,
    reason: RemovalReason,
) -> Option<FeatureId> {
    match reason {
        RemovalReason::Merged { into } => Some(FeatureId::new(producer_instance_id, into)),
        RemovalReason::Expired => None,
        _ => None,
    }
}

//! Typed aircraft feature changes.

use surveillance_core::{
    ProducerInstanceId, RemovalReason, SnapshotRevision, TimedField, TrackDelta, TrackId,
    TrackSnapshot, TrackSnapshotHandle, Wgs84Position,
};

#[cfg(test)]
mod tests;

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
        /// Stable identity of the feature.
        id: TrackId,
    },
    /// Remove an existing feature.
    Remove {
        /// Stable identity of the feature.
        id: TrackId,
        /// Track that absorbed this feature, if a merge caused removal.
        transfer_to: Option<TrackId>,
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

    /// Stable identity of this feature.
    #[must_use]
    pub fn id(&self) -> TrackId {
        self.snapshot.id
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
        TrackDelta::Created(snapshot) | TrackDelta::Updated(snapshot) => map_upsert(snapshot),
        TrackDelta::Coasting(snapshot) => snapshot
            .position
            .map(|_| FeatureDelta::Stale { id: snapshot.id }),
        TrackDelta::Removed { id, reason, .. } => Some(FeatureDelta::Remove {
            id: *id,
            transfer_to: transfer_target(*reason),
        }),
        _ => None,
    }
}

fn map_upsert(snapshot: &TrackSnapshotHandle) -> Option<FeatureDelta> {
    snapshot
        .position
        .map(|position| FeatureDelta::Upsert(AircraftFeature::new(position, snapshot.clone())))
}

fn transfer_target(reason: RemovalReason) -> Option<TrackId> {
    match reason {
        RemovalReason::Merged { into } => Some(into),
        RemovalReason::Expired => None,
        _ => None,
    }
}

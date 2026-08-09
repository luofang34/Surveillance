//! Producer identity and revision assignment for published track state.

use thiserror::Error;

use crate::track::{
    ProducerInstanceId, RemovalReason, SnapshotRevision, TrackDelta, TrackId, TrackSnapshotHandle,
    TrackState,
};

/// Assign revisions to published states from one engine.
pub(crate) struct SnapshotPublisher {
    producer_instance_id: ProducerInstanceId,
}

impl SnapshotPublisher {
    pub(crate) fn new(producer_instance_id: ProducerInstanceId) -> Self {
        Self {
            producer_instance_id,
        }
    }

    pub(crate) fn ensure_capacity(
        &self,
        state: &TrackState,
        publications: u8,
    ) -> Result<(), SnapshotPublicationError> {
        if state
            .snapshot_revision
            .checked_advance(publications)
            .is_some()
        {
            Ok(())
        } else {
            Err(self.exhausted(state, publications))
        }
    }

    pub(crate) fn revise(
        &self,
        state: &mut TrackState,
    ) -> Result<SnapshotRevision, SnapshotPublicationError> {
        let Some(next) = state.snapshot_revision.next() else {
            return Err(self.exhausted(state, 1));
        };
        state.snapshot_revision = next;
        Ok(next)
    }

    pub(crate) fn snapshot(
        &self,
        state: &mut TrackState,
    ) -> Result<TrackSnapshotHandle, SnapshotPublicationError> {
        self.revise(state)?;
        Ok(state.handle(self.producer_instance_id))
    }

    pub(crate) fn removal(
        &self,
        state: &mut TrackState,
        reason: RemovalReason,
    ) -> Result<TrackDelta, SnapshotPublicationError> {
        self.revise(state)?;
        Ok(TrackDelta::Removed {
            producer_instance_id: self.producer_instance_id,
            snapshot_revision: state.snapshot_revision,
            id: state.snapshot.id,
            key: state.snapshot.key,
            reason,
        })
    }

    fn exhausted(&self, state: &TrackState, publications: u8) -> SnapshotPublicationError {
        SnapshotPublicationError::RevisionExhausted {
            producer_instance_id: self.producer_instance_id,
            track_id: state.snapshot.id,
            current_revision: state.snapshot_revision,
            required_publications: publications,
        }
    }
}

/// Failure to publish a new track state.
///
/// This error ends the producer instance. The caller must construct a new
/// engine with a new producer identity before it publishes more states.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum SnapshotPublicationError {
    /// The track has no revision value available for the requested states.
    #[error(
        "track {track_id:?} cannot publish {required_publications} state(s) after revision {current_revision:?} for producer {producer_instance_id:?}"
    )]
    RevisionExhausted {
        /// Producer that owns the revision sequence.
        producer_instance_id: ProducerInstanceId,
        /// Subject whose revision sequence is exhausted.
        track_id: TrackId,
        /// Last revision the producer published for the subject.
        current_revision: SnapshotRevision,
        /// Number of new states the operation must publish.
        required_publications: u8,
    },
}

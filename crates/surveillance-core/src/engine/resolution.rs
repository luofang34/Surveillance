//! Track lookup and creation for classified wire identities.

use crate::observation::TrafficObservation;
use crate::track::{TrackId, TrackKey, TrackSnapshot, TrackState};

use super::publication::SnapshotPublisher;
use super::{IngestError, SurveillanceEngine};

impl SurveillanceEngine {
    /// Find the track for an observation, or create one.
    pub(super) fn resolve_track(
        &mut self,
        observation: &TrafficObservation,
        classified_key: Option<TrackKey>,
    ) -> Result<(TrackId, bool), IngestError> {
        let observed_id = self.keys.get(&observation.key).copied();
        let classified_id = classified_key.and_then(|key| self.keys.get(&key).copied());
        if let (Some(observed), Some(classified)) = (observed_id, classified_id)
            && observed != classified
        {
            return Err(IngestError::IdentityClassificationConflict {
                observed: observation.key,
                classified: classified_key.unwrap_or(observation.key),
                observed_id: observed,
                classified_id: classified,
            });
        }

        if let Some(id) = observed_id.or(classified_id) {
            let state = self
                .tracks
                .get(&id)
                .ok_or(IngestError::TrackStateMissing { id })?;
            SnapshotPublisher::new(self.producer_instance_id).ensure_capacity(state, 1)?;
            self.keys.insert(observation.key, id);
            return Ok((id, false));
        }
        if let Some(capacity) = self.config.max_tracks
            && self.tracks.len() >= capacity
        {
            return Err(IngestError::TrackCapacityExceeded { capacity });
        }

        let id = TrackId::new(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        let mut snapshot =
            TrackSnapshot::new(id, observation.key, observation.time.received_at_micros);
        snapshot.ownship_shadow = self.ownship.contains(&observation.key);
        self.tracks.insert(id, TrackState::from(snapshot));
        self.keys.insert(observation.key, id);
        Ok((id, true))
    }
}

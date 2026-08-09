//! Applying one normalized observation to a track's fused state.
//!
//! Every field is offered to [`crate::fusion::fuse`] with its own freshness
//! window, so precedence and ordering tolerance live in one place and a
//! callsign update cannot refresh an old position. The return value reports
//! whether a consumer-visible value changed; provenance-only refreshes stay
//! silent so duplicate receptions from several receivers emit no deltas.

use crate::freshness::{FieldProvenance, FreshnessPolicy, TimedField};
use crate::fusion::{FieldUpdate, fuse};
use crate::observation::TrafficObservation;
use crate::track::TrackSnapshot;

/// Apply every non-position field, reporting whether a consumer-visible value
/// changed.
pub(crate) fn apply_fields(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> FieldChanges {
    let mut changes = apply_altitudes(snapshot, observation, policy);
    changes.include(apply_velocity(snapshot, observation, policy));
    changes.merge(apply_identity(snapshot, observation, policy));
    changes.merge(apply_status(snapshot, observation, policy));
    changes
}

/// Changes to exported fields from one observation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FieldChanges {
    applied: bool,
    meaningful: bool,
}

impl FieldChanges {
    pub(crate) fn include(&mut self, update: FieldUpdate) {
        self.applied |= update.was_applied();
        self.meaningful |= update.is_meaningful();
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.applied |= other.applied;
        self.meaningful |= other.meaningful;
    }

    pub(crate) fn was_applied(self) -> bool {
        self.applied
    }

    pub(crate) fn is_meaningful(self) -> bool {
        self.meaningful
    }
}

/// Bind one measured value to the observation that supplied it.
pub(crate) fn timed<T>(observation: &TrafficObservation, value: T) -> TimedField<T> {
    TimedField::new(
        value,
        observation.time,
        observation.quality,
        FieldProvenance::new(
            observation.origin,
            observation.key.namespace,
            observation.source,
        ),
    )
}

fn apply_altitudes(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> FieldChanges {
    let mut changes = FieldChanges::default();
    if let Some(altitude) = observation.pressure_altitude_ft {
        changes.include(fuse(
            &mut snapshot.pressure_altitude_ft,
            timed(observation, altitude),
            policy.position_fresh_micros,
        ));
    }
    if let Some(altitude) = observation.geometric_altitude_ft {
        changes.include(fuse(
            &mut snapshot.geometric_altitude_ft,
            timed(observation, altitude),
            policy.position_fresh_micros,
        ));
    }
    changes
}

fn apply_velocity(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> FieldUpdate {
    let Some(velocity) = observation.velocity else {
        return FieldUpdate::Rejected;
    };
    fuse(
        &mut snapshot.velocity,
        timed(observation, velocity),
        policy.velocity_fresh_micros,
    )
}

fn apply_identity(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> FieldChanges {
    let mut changes = FieldChanges::default();
    if let Some(callsign) = observation.callsign.as_ref() {
        changes.include(fuse(
            &mut snapshot.callsign,
            timed(observation, callsign.clone()),
            policy.identity_fresh_micros,
        ));
    }
    if let Some(squawk) = observation.squawk {
        changes.include(fuse(
            &mut snapshot.squawk,
            timed(observation, squawk),
            policy.identity_fresh_micros,
        ));
    }
    changes
}

fn apply_status(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> FieldChanges {
    let mut changes = FieldChanges::default();
    if let Some(air_ground) = observation.air_ground {
        changes.include(fuse(
            &mut snapshot.air_ground,
            timed(observation, air_ground),
            policy.identity_fresh_micros,
        ));
    }
    if let Some(emergency) = observation.emergency {
        changes.include(fuse(
            &mut snapshot.emergency,
            timed(observation, emergency),
            policy.identity_fresh_micros,
        ));
    }
    changes
}

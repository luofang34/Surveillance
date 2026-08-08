//! Applying one normalized observation to a track's fused state.
//!
//! Every field is offered to [`crate::fusion::fuse`] with its own freshness
//! window, so precedence and ordering tolerance live in one place and a
//! callsign update cannot refresh an old position. The return value reports
//! whether a consumer-visible value changed; provenance-only refreshes stay
//! silent so duplicate receptions from several receivers emit no deltas.

use crate::freshness::{FieldProvenance, FreshnessPolicy, TimedField};
use crate::fusion::fuse;
use crate::observation::TrafficObservation;
use crate::track::TrackSnapshot;

/// Apply every non-position field, reporting whether a consumer-visible value
/// changed.
pub(crate) fn apply_fields(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> bool {
    let mut changed = apply_altitudes(snapshot, observation, policy);
    changed |= apply_velocity(snapshot, observation, policy);
    changed |= apply_identity(snapshot, observation, policy);
    changed |= apply_status(snapshot, observation, policy);
    changed
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
) -> bool {
    let mut changed = false;
    if let Some(altitude) = observation.pressure_altitude_ft {
        changed |= fuse(
            &mut snapshot.pressure_altitude_ft,
            timed(observation, altitude),
            policy.position_fresh_micros,
        )
        .is_meaningful();
    }
    if let Some(altitude) = observation.geometric_altitude_ft {
        changed |= fuse(
            &mut snapshot.geometric_altitude_ft,
            timed(observation, altitude),
            policy.position_fresh_micros,
        )
        .is_meaningful();
    }
    changed
}

fn apply_velocity(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> bool {
    let Some(velocity) = observation.velocity else {
        return false;
    };
    fuse(
        &mut snapshot.velocity,
        timed(observation, velocity),
        policy.velocity_fresh_micros,
    )
    .is_meaningful()
}

fn apply_identity(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> bool {
    let mut changed = false;
    if let Some(callsign) = observation.callsign.as_ref() {
        changed |= fuse(
            &mut snapshot.callsign,
            timed(observation, callsign.clone()),
            policy.identity_fresh_micros,
        )
        .is_meaningful();
    }
    if let Some(squawk) = observation.squawk {
        changed |= fuse(
            &mut snapshot.squawk,
            timed(observation, squawk),
            policy.identity_fresh_micros,
        )
        .is_meaningful();
    }
    changed
}

fn apply_status(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> bool {
    let mut changed = false;
    if let Some(air_ground) = observation.air_ground {
        changed |= fuse(
            &mut snapshot.air_ground,
            timed(observation, air_ground),
            policy.identity_fresh_micros,
        )
        .is_meaningful();
    }
    if let Some(emergency) = observation.emergency {
        changed |= fuse(
            &mut snapshot.emergency,
            timed(observation, emergency),
            policy.identity_fresh_micros,
        )
        .is_meaningful();
    }
    changed
}

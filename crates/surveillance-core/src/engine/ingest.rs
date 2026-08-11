//! Applying one normalized observation to a track's fused state.
//!
//! Every field is offered to [`crate::fusion::fuse`] with its own freshness
//! window, so precedence and ordering tolerance live in one place and a
//! callsign update cannot refresh an old position. The return value reports
//! whether a consumer-visible value changed. A source handover emits a delta
//! so a delta-only consumer keeps current provenance. Repeated content from
//! one source stays silent.

use crate::engine::{FieldDisposition, FieldOutcomes};
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
) -> FieldOutcomes {
    FieldOutcomes {
        pressure_altitude_ft: observation.pressure_altitude_ft.map_or(
            FieldDisposition::NotSupplied,
            |value| {
                disposition(fuse(
                    &mut snapshot.pressure_altitude_ft,
                    timed(observation, value),
                    policy.position_fresh_micros,
                ))
            },
        ),
        geometric_altitude_ft: observation.geometric_altitude_ft.map_or(
            FieldDisposition::NotSupplied,
            |value| {
                disposition(fuse(
                    &mut snapshot.geometric_altitude_ft,
                    timed(observation, value),
                    policy.position_fresh_micros,
                ))
            },
        ),
        velocity: apply_velocity(snapshot, observation, policy),
        callsign: apply_callsign(snapshot, observation, policy),
        squawk: observation
            .squawk
            .map_or(FieldDisposition::NotSupplied, |value| {
                disposition(fuse(
                    &mut snapshot.squawk,
                    timed(observation, value),
                    policy.identity_fresh_micros,
                ))
            }),
        air_ground: observation
            .air_ground
            .map_or(FieldDisposition::NotSupplied, |value| {
                disposition(fuse(
                    &mut snapshot.air_ground,
                    timed(observation, value),
                    policy.identity_fresh_micros,
                ))
            }),
        emergency: observation
            .emergency
            .map_or(FieldDisposition::NotSupplied, |value| {
                disposition(fuse(
                    &mut snapshot.emergency,
                    timed(observation, value),
                    policy.identity_fresh_micros,
                ))
            }),
        emitter_category: observation.emitter_category.map_or(
            FieldDisposition::NotSupplied,
            |value| {
                disposition(fuse(
                    &mut snapshot.emitter_category,
                    timed(observation, value),
                    policy.identity_fresh_micros,
                ))
            },
        ),
        adsb_integrity: FieldDisposition::NotSupplied,
        adsb_position_integrity: FieldDisposition::NotSupplied,
    }
}

/// Bind one measured value to the observation that supplied it.
pub(crate) fn timed<T>(observation: &TrafficObservation, value: T) -> TimedField<T> {
    timed_with_quality(observation, value, observation.quality)
}

/// Bind one value to its observation with derived quality.
pub(crate) fn timed_with_quality<T>(
    observation: &TrafficObservation,
    value: T,
    quality: crate::freshness::FieldQuality,
) -> TimedField<T> {
    TimedField::new(
        value,
        observation.time,
        quality,
        FieldProvenance::new(
            observation.origin,
            observation.key.namespace,
            observation.source,
        ),
    )
}

fn apply_velocity(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> FieldDisposition {
    let Some(velocity) = observation.velocity else {
        return FieldDisposition::NotSupplied;
    };
    disposition(fuse(
        &mut snapshot.velocity,
        timed(observation, velocity),
        policy.velocity_fresh_micros,
    ))
}

fn apply_callsign(
    snapshot: &mut TrackSnapshot,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> FieldDisposition {
    let Some(callsign) = observation.callsign.as_ref() else {
        return FieldDisposition::NotSupplied;
    };
    disposition(fuse(
        &mut snapshot.callsign,
        timed(observation, callsign.clone()),
        policy.identity_fresh_micros,
    ))
}

fn disposition(update: FieldUpdate) -> FieldDisposition {
    FieldDisposition::from_update(update)
}

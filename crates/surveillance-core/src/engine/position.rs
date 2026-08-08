//! Turning an observation's position content into a fused track position.
//!
//! Decoded positions enter fusion directly. Airborne CPR frames decode
//! globally from the track's even/odd history. Local decoding is not a
//! fallback for a pair that decoded into inconsistent zones or failed its
//! plausibility gate — a disagreeing pair is evidence something is wrong, and
//! resolving it against a reference would launder that away. It applies only
//! where no pair exists to decode from: a track whose position is already
//! established and fresh can resolve a single frame against its own last
//! position, and surface frames, whose zones repeat once per quadrant, decode
//! this way or not at all.

use crate::cpr::{
    AIRBORNE_DEGREES_PER_CYCLE, CprDecodeError, CprFrame, CprPolicy, SURFACE_DEGREES_PER_CYCLE,
    local, validate_range,
};
use crate::engine::ingest::timed;
use crate::freshness::FreshnessPolicy;
use crate::fusion::{FieldUpdate, fuse};
use crate::observation::{PositionObservation, TrafficObservation, Wgs84Position};
use crate::track::TrackState;

/// Everything the position path needs beyond the observation itself.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PositionContext<'a> {
    pub(crate) freshness: &'a FreshnessPolicy,
    pub(crate) cpr: &'a CprPolicy,
    /// Caller-supplied position used to resolve reference-dependent frames
    /// for a track that has no position of its own yet.
    pub(crate) reference: Option<Wgs84Position>,
}

/// Apply an observation's position content.
///
/// The outcome separates "the track's position advanced" from "a consumer
/// would see a different value": correlation evidence accumulates on every
/// accepted position, including one that repeats the previous value, while
/// deltas are emitted only for a changed value.
pub(crate) fn apply(
    state: &mut TrackState,
    observation: &TrafficObservation,
    context: PositionContext<'_>,
) -> FieldUpdate {
    let Some(position) = observation.position else {
        return FieldUpdate::Rejected;
    };

    let decoded = match position {
        PositionObservation::Decoded(decoded) => Ok(decoded),
        PositionObservation::CprAirborne(frame) => {
            decode_airborne(state, observation, frame, context)
        }
        PositionObservation::CprSurface(frame) => {
            decode_surface(state, observation, frame, context)
        }
    };

    let Ok(candidate) = decoded else {
        return FieldUpdate::Rejected;
    };
    if validate_against_previous(state, observation, candidate, context).is_err() {
        return FieldUpdate::Rejected;
    }

    fuse(
        &mut state.snapshot.position,
        timed(observation, candidate),
        context.freshness.position_fresh_micros,
    )
}

fn decode_airborne(
    state: &mut TrackState,
    observation: &TrafficObservation,
    frame: CprFrame,
    context: PositionContext<'_>,
) -> Result<Wgs84Position, CprDecodeError> {
    state.cpr.record(frame, observation.time.observed_at_micros);
    match state.cpr.decode_global(context.cpr) {
        Err(CprDecodeError::PairIncomplete) => {
            let reference = established_position(state, observation, context)
                .ok_or(CprDecodeError::PairIncomplete)?;
            Ok(local::decode(frame, reference, AIRBORNE_DEGREES_PER_CYCLE))
        }
        outcome => outcome,
    }
}

fn decode_surface(
    state: &TrackState,
    observation: &TrafficObservation,
    frame: CprFrame,
    context: PositionContext<'_>,
) -> Result<Wgs84Position, CprDecodeError> {
    let reference = established_position(state, observation, context)
        .ok_or(CprDecodeError::ReferenceRequired)?;
    Ok(local::decode(frame, reference, SURFACE_DEGREES_PER_CYCLE))
}

/// The track's own fresh position, else the caller-supplied reference.
fn established_position(
    state: &TrackState,
    observation: &TrafficObservation,
    context: PositionContext<'_>,
) -> Option<Wgs84Position> {
    state
        .snapshot
        .position
        .as_ref()
        .filter(|position| {
            position.is_fresh(
                observation.time.received_at_micros,
                context.freshness.position_fresh_micros,
            )
        })
        .map(|position| position.value)
        .or(context.reference)
}

fn validate_against_previous(
    state: &TrackState,
    observation: &TrafficObservation,
    candidate: Wgs84Position,
    context: PositionContext<'_>,
) -> Result<(), CprDecodeError> {
    let Some(previous) = state.snapshot.position.as_ref() else {
        return Ok(());
    };
    // Travel is bounded by the interval between the two measurements. Using
    // arrival here, or the pessimistic age that widens with uncertainty,
    // would inflate the budget and admit exactly the implausible jumps the
    // gate exists to refuse.
    let elapsed_micros = observation
        .time
        .observed_at_micros
        .saturating_sub(previous.observed_at_micros());
    validate_range(candidate, previous.value, elapsed_micros, context.cpr)
}

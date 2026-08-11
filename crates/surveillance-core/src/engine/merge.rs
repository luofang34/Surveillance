//! Joining two tracks that correlation has decided are one aircraft.
//!
//! A merge preserves history rather than restarting it. The earlier-created
//! track keeps its [`TrackId`]. It absorbs the other track's identities and
//! fused fields through the normal precedence rules. It then re-keys to the
//! most authoritative identity. The absorbed track is removed with
//! [`RemovalReason::Merged`], which tells a consumer to transfer what it was
//! displaying rather than delete it.

use alloc::collections::BTreeMap;

use crate::association::{forget_peer, namespace_rank};
use crate::engine::publication::{SnapshotPublicationError, SnapshotPublisher};
use crate::freshness::FreshnessPolicy;
use crate::fusion::fuse;
use crate::track::{
    RemovalReason, TrackDelta, TrackId, TrackKey, TrackPhase, TrackSnapshot, TrackState,
};

use super::integrity;

/// Which of two correlated tracks keeps its identity.
///
/// The track a consumer has been watching longest survives; equal creation
/// times fall back to identity order so a merge is never order-dependent.
#[must_use]
pub(crate) fn survivor_of(left: &TrackState, right: &TrackState) -> TrackId {
    let left_key = (left.snapshot.created_at_micros, left.snapshot.id);
    let right_key = (right.snapshot.created_at_micros, right.snapshot.id);
    if left_key <= right_key {
        left.snapshot.id
    } else {
        right.snapshot.id
    }
}

/// Absorb `absorbed` into `survivor`, emitting the removal and the re-keyed
/// survivor.
pub(crate) fn merge(
    tracks: &mut BTreeMap<TrackId, TrackState>,
    keys: &mut BTreeMap<TrackKey, TrackId>,
    survivor: TrackId,
    absorbed: TrackId,
    freshness: &FreshnessPolicy,
    publisher: &SnapshotPublisher,
    on_delta: &mut impl FnMut(&TrackDelta),
) -> Result<(), SnapshotPublicationError> {
    let (Some(absorbed_state), Some(survivor_state)) =
        (tracks.get(&absorbed), tracks.get(&survivor))
    else {
        return Ok(());
    };
    publisher.ensure_capacity(absorbed_state, 1)?;
    publisher.ensure_capacity(survivor_state, 1)?;

    let Some(mut absorbed_state) = tracks.remove(&absorbed) else {
        return Ok(());
    };
    for retained in tracks.values_mut() {
        forget_peer(&mut retained.correlation, absorbed);
    }
    let Some(survivor_state) = tracks.get_mut(&survivor) else {
        tracks.insert(absorbed, absorbed_state);
        return Ok(());
    };

    adopt_identities(&mut survivor_state.snapshot, &absorbed_state.snapshot);
    adopt_fields(
        &mut survivor_state.snapshot,
        &absorbed_state.snapshot,
        freshness,
    );
    integrity::adopt(survivor_state, &absorbed_state, freshness);
    survivor_state.snapshot.created_at_micros = survivor_state
        .snapshot
        .created_at_micros
        .min(absorbed_state.snapshot.created_at_micros);
    survivor_state.snapshot.last_observed_at_micros = survivor_state
        .snapshot
        .last_observed_at_micros
        .max(absorbed_state.snapshot.last_observed_at_micros);
    survivor_state.last_received_at_micros = survivor_state
        .last_received_at_micros
        .max(absorbed_state.last_received_at_micros);
    if absorbed_state.snapshot.phase == TrackPhase::Active {
        survivor_state.snapshot.phase = TrackPhase::Active;
    }
    survivor_state.snapshot.ownship_shadow |= absorbed_state.snapshot.ownship_shadow;
    survivor_state
        .identity_activity
        .extend_from_slice(&absorbed_state.identity_activity);
    for key in identities_of(&survivor_state.snapshot) {
        keys.insert(key, survivor);
    }

    let removed = publisher.removal(
        &mut absorbed_state,
        RemovalReason::Merged { into: survivor },
    )?;
    on_delta(&removed);
    let updated = TrackDelta::Updated(publisher.snapshot(survivor_state)?);
    on_delta(&updated);
    Ok(())
}

/// Take on every identity of the absorbed track and re-key to the most
/// authoritative one.
fn adopt_identities(survivor: &mut TrackSnapshot, absorbed: &TrackSnapshot) {
    survivor.identities.associate(absorbed.identities.primary());
    for key in absorbed.identities.associated() {
        survivor.identities.associate(*key);
    }

    let best = identities_of(survivor)
        .into_iter()
        .max_by_key(|key| (namespace_rank(key.namespace), core::cmp::Reverse(*key)));
    if let Some(best) = best {
        survivor.identities.promote(best);
        survivor.key = best;
    }
}

/// Every identity the track currently answers to.
fn identities_of(snapshot: &TrackSnapshot) -> alloc::vec::Vec<TrackKey> {
    let mut keys = alloc::vec::Vec::with_capacity(snapshot.identities.associated().len() + 2);
    keys.push(snapshot.key);
    keys.push(snapshot.identities.primary());
    keys.extend_from_slice(snapshot.identities.associated());
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// Offer the absorbed track's fused fields to the survivor, so the same
/// quality, link, and age precedence that governs one track's fields governs
/// the join.
fn adopt_fields(
    survivor: &mut TrackSnapshot,
    absorbed: &TrackSnapshot,
    freshness: &FreshnessPolicy,
) {
    if let Some(position) = absorbed.position {
        fuse(
            &mut survivor.position,
            position,
            freshness.position_fresh_micros,
        );
    }
    if let Some(altitude) = absorbed.pressure_altitude_ft {
        fuse(
            &mut survivor.pressure_altitude_ft,
            altitude,
            freshness.position_fresh_micros,
        );
    }
    if let Some(altitude) = absorbed.geometric_altitude_ft {
        fuse(
            &mut survivor.geometric_altitude_ft,
            altitude,
            freshness.position_fresh_micros,
        );
    }
    if let Some(velocity) = absorbed.velocity {
        fuse(
            &mut survivor.velocity,
            velocity,
            freshness.velocity_fresh_micros,
        );
    }
    adopt_identity_fields(survivor, absorbed, freshness);
}

fn adopt_identity_fields(
    survivor: &mut TrackSnapshot,
    absorbed: &TrackSnapshot,
    freshness: &FreshnessPolicy,
) {
    let window = freshness.identity_fresh_micros;
    if let Some(callsign) = absorbed.callsign.clone() {
        fuse(&mut survivor.callsign, callsign, window);
    }
    if let Some(squawk) = absorbed.squawk {
        fuse(&mut survivor.squawk, squawk, window);
    }
    if let Some(air_ground) = absorbed.air_ground {
        fuse(&mut survivor.air_ground, air_ground, window);
    }
    if let Some(emergency) = absorbed.emergency {
        fuse(&mut survivor.emergency, emergency, window);
    }
    if let Some(emitter_category) = absorbed.emitter_category {
        fuse(&mut survivor.emitter_category, emitter_category, window);
    }
}

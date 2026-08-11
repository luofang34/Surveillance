//! Deterministic coasting and expiration.
//!
//! Lifecycle transitions are a pure function of each track's newest applied
//! observation and the caller-supplied monotonic time. Nothing here reads a
//! clock or starts a timer, so an identical observation sequence replayed
//! with identical `advance_time` calls produces an identical delta stream.
//!
//! Track age saturates at zero, so an `advance_time` call carrying a time
//! earlier than a track's newest observation transitions nothing rather than
//! aging it backwards.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::association::{CorrelationPolicy, forget_peer};
use crate::engine::EngineConfig;
use crate::engine::publication::{SnapshotPublicationError, SnapshotPublisher};
use crate::track::{RemovalReason, TrackDelta, TrackId, TrackKey, TrackPhase, TrackState};

/// Apply demotion, coasting, and removal at `now_micros`, emitting one delta
/// per transition in [`TrackId`] order.
pub(crate) fn advance(
    tracks: &mut BTreeMap<TrackId, TrackState>,
    keys: &mut BTreeMap<TrackKey, TrackId>,
    now_micros: u64,
    config: &EngineConfig,
    publisher: &SnapshotPublisher,
    on_delta: &mut impl FnMut(&TrackDelta),
) -> Result<(), SnapshotPublicationError> {
    let policy = &config.freshness;
    let mut expired: Vec<TrackId> = Vec::new();

    for state in tracks.values() {
        publisher.ensure_capacity(state, publication_count(state, now_micros, config))?;
    }

    for (id, state) in tracks.iter_mut() {
        if demote_silent_primary(state, now_micros, &config.correlation) {
            keys.insert(state.snapshot.key, *id);
            let delta = TrackDelta::Updated(publisher.snapshot(state)?);
            on_delta(&delta);
        }

        let age_micros = now_micros.saturating_sub(state.snapshot.last_observed_at_micros);
        if age_micros > policy.remove_after_micros {
            expired.push(*id);
        } else if age_micros > policy.coast_after_micros
            && state.snapshot.phase == TrackPhase::Active
        {
            state.snapshot.phase = TrackPhase::Coasting;
            let delta = TrackDelta::Coasting(publisher.snapshot(state)?);
            on_delta(&delta);
        }
    }

    for id in expired {
        let Some(mut state) = tracks.remove(&id) else {
            continue;
        };
        for retained in tracks.values_mut() {
            forget_peer(&mut retained.correlation, id);
        }
        let delta = publisher.removal(&mut state, RemovalReason::Expired)?;
        release_keys(keys, &state);
        on_delta(&delta);
    }
    Ok(())
}

fn publication_count(state: &TrackState, now_micros: u64, config: &EngineConfig) -> u8 {
    let demotion =
        u8::from(silent_primary_successor(state, now_micros, &config.correlation).is_some());
    let age_micros = now_micros.saturating_sub(state.snapshot.last_observed_at_micros);
    let lifecycle = u8::from(
        age_micros > config.freshness.remove_after_micros
            || (age_micros > config.freshness.coast_after_micros
                && state.snapshot.phase == TrackPhase::Active),
    );
    demotion + lifecycle
}

/// Re-key a track whose primary identity has gone silent while another of its
/// identities keeps reporting.
///
/// This is the reverse of promotion and preserves just as much: the track
/// keeps its [`TrackId`], its fused history, and every identity it has ever
/// answered to. Only the identity it presents changes, so a cooperative
/// target that stops transmitting continues as the non-cooperative track that
/// still sees it, rather than vanishing and reappearing.
fn demote_silent_primary(
    state: &mut TrackState,
    now_micros: u64,
    policy: &CorrelationPolicy,
) -> bool {
    let Some(successor) = silent_primary_successor(state, now_micros, policy) else {
        return false;
    };
    state.snapshot.identities.promote(successor);
    state.snapshot.key = successor;
    true
}

fn silent_primary_successor(
    state: &TrackState,
    now_micros: u64,
    policy: &CorrelationPolicy,
) -> Option<TrackKey> {
    let primary = state.snapshot.identities.primary();
    let primary_seen = state.identity_last_seen(primary).unwrap_or(0);
    if now_micros.saturating_sub(primary_seen) <= policy.demote_after_micros {
        return None;
    }

    state
        .snapshot
        .identities
        .associated()
        .iter()
        .filter_map(|key| state.identity_last_seen(*key).map(|seen| (seen, *key)))
        .filter(|(seen, _)| *seen > primary_seen)
        .max()
        .map(|(_, key)| key)
}

/// Release every identity the removed track occupied, so a later observation
/// for any of them starts a new track rather than resurrecting a dangling
/// index entry.
fn release_keys(keys: &mut BTreeMap<TrackKey, TrackId>, state: &TrackState) {
    keys.remove(&state.snapshot.key);
    keys.remove(&state.snapshot.identities.primary());
    for associated in state.snapshot.identities.associated() {
        keys.remove(associated);
    }
}

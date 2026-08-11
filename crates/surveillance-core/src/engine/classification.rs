//! Identity classification from source-neutral emitter categories.

use crate::association::namespace_rank;
use crate::observation::{EmitterCategory, TrafficObservation};
use crate::track::{AddressNamespace, TrackKey, TrackState};

use super::is_ownship_shadow;

/// Get the classified key for one unclassified ADS-B key.
pub(super) fn classified_key(key: TrackKey, category: EmitterCategory) -> Option<TrackKey> {
    if key.namespace != AddressNamespace::AdsbNonIcao {
        return None;
    }

    let namespace = if category.is_surface_vehicle() {
        AddressNamespace::SurfaceVehicle
    } else if category.is_fixed_obstacle() {
        AddressNamespace::FixedBeacon
    } else {
        AddressNamespace::SelfAssigned
    };
    Some(TrackKey::new(namespace, key.address))
}

/// Apply the fused category to the identity set.
///
/// The wire key stays associated with the classified key. A stronger
/// identity from correlation stays primary.
pub(super) fn apply(
    state: &mut TrackState,
    observation: &TrafficObservation,
    ownship: &[TrackKey],
) -> Option<TrackKey> {
    let category = state.snapshot.emitter_category.as_ref()?.value;
    let target = classified_key(observation.key, category)?;
    state.mark_identity_seen(target, observation.time.received_at_micros);

    let target_was_known = state.snapshot.identities.contains(target);
    state.snapshot.identities.associate(target);
    let primary = state.snapshot.identities.primary();
    let should_promote = primary.namespace == AddressNamespace::AdsbNonIcao
        || namespace_rank(target.namespace) > namespace_rank(primary.namespace);
    if should_promote {
        state.snapshot.identities.promote(target);
        state.snapshot.key = target;
    }

    let shadow = is_ownship_shadow(&state.snapshot, ownship);
    let shadow_changed = state.snapshot.ownship_shadow != shadow;
    state.snapshot.ownship_shadow = shadow;
    (!target_was_known || should_promote || shadow_changed).then_some(target)
}

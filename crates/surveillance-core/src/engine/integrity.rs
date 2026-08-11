//! Retained ADS-B integrity declarations and position quality derivation.

use crate::engine::FieldDisposition;
use crate::engine::ingest::timed;
use crate::freshness::{FieldQuality, FreshnessPolicy, TimedField};
use crate::fusion::fuse;
use crate::observation::{AdsbIntegrityDeclaration, TrafficObservation};
use crate::track::TrackState;

/// Internal outcomes and any exported position metadata change.
pub(super) struct IntegrityApply {
    pub(super) declaration: FieldDisposition,
    pub(super) position_code: FieldDisposition,
    pub(super) position_metadata_changed: bool,
}

/// Retain supplied declarations before the position path uses them.
pub(super) fn apply(
    state: &mut TrackState,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> IntegrityApply {
    let declaration = observation
        .adsb_integrity
        .map_or(FieldDisposition::NotSupplied, |value| {
            FieldDisposition::from_update(fuse(
                &mut state.adsb_integrity,
                timed(observation, value),
                policy.integrity_fresh_micros,
            ))
        });
    let position_code =
        observation
            .adsb_position_integrity
            .map_or(FieldDisposition::NotSupplied, |value| {
                FieldDisposition::from_update(fuse(
                    &mut state.adsb_position_integrity,
                    timed(observation, value),
                    policy.position_fresh_micros,
                ))
            });
    let position_metadata_changed = declaration.was_applied()
        && refresh_position_quality(state, observation.time.received_at_micros, policy);
    IntegrityApply {
        declaration,
        position_code,
        position_metadata_changed,
    }
}

/// Add a retained declaration to one new position candidate.
pub(super) fn quality_for_position(
    state: &TrackState,
    observation: &TrafficObservation,
    policy: &FreshnessPolicy,
) -> FieldQuality {
    let mut quality = observation.quality;
    let (Some(declaration), Some(position_code)) = (
        state.adsb_integrity.as_ref(),
        observation.adsb_position_integrity,
    ) else {
        return quality;
    };
    if !declaration.is_fresh(
        observation.time.received_at_micros,
        policy.integrity_fresh_micros,
    ) || !same_transmission_identity(declaration, observation)
    {
        return quality;
    }

    let derived = declaration.value.quality_for(position_code);
    quality.nic = derived.nic.or(quality.nic);
    quality.nac_p = derived.nac_p.or(quality.nac_p);
    quality.sil = derived.sil.or(quality.sil);
    quality
}

/// Retain integrity state across a correlation merge.
pub(super) fn adopt(survivor: &mut TrackState, absorbed: &TrackState, policy: &FreshnessPolicy) {
    if let Some(declaration) = absorbed.adsb_integrity {
        fuse(
            &mut survivor.adsb_integrity,
            declaration,
            policy.integrity_fresh_micros,
        );
    }
    if let Some(position) = absorbed.adsb_position_integrity {
        fuse(
            &mut survivor.adsb_position_integrity,
            position,
            policy.position_fresh_micros,
        );
    }
}

fn refresh_position_quality(
    state: &mut TrackState,
    now_micros: u64,
    policy: &FreshnessPolicy,
) -> bool {
    let (Some(declaration), Some(position_code), Some(position)) = (
        state.adsb_integrity.as_ref(),
        state.adsb_position_integrity.as_ref(),
        state.snapshot.position.as_mut(),
    ) else {
        return false;
    };
    if !declaration.is_fresh(now_micros, policy.integrity_fresh_micros)
        || declaration.provenance.origin != position_code.provenance.origin
        || declaration.provenance.namespace != position_code.provenance.namespace
        || position.time != position_code.time
        || position.provenance != position_code.provenance
    {
        return false;
    }

    let derived = declaration.value.quality_for(position_code.value);
    let mut quality = position.quality;
    quality.nic = derived.nic;
    quality.nac_p = derived.nac_p;
    quality.sil = derived.sil;
    if position.quality == quality {
        return false;
    }
    position.quality = quality;
    true
}

fn same_transmission_identity(
    declaration: &TimedField<AdsbIntegrityDeclaration>,
    observation: &TrafficObservation,
) -> bool {
    declaration.provenance.origin == observation.origin
        && declaration.provenance.namespace == observation.key.namespace
}

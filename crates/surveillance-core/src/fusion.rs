//! Cross-link field fusion policy.
//!
//! Fusion decides which observation's value a fused field keeps when the same
//! target is heard through several links, bands, or receivers. The decision
//! procedure is per field and evaluates, in order:
//!
//! 1. A strictly older observation never replaces a newer applied value, so
//!    a delayed reception cannot undo a current one.
//! 2. A value whose age exceeds its freshness window is stale, and any
//!    candidate replaces it regardless of link. A fresh rebroadcast therefore
//!    beats a stale direct value.
//! 3. Between fresh values, the higher [`link_rank`] wins: the target's own
//!    transmissions outrank ground rebroadcasts.
//! 4. At equal rank, the newer observation wins; two observations of the same
//!    instant are resolved by quality, where better means no indicator is
//!    worse and at least one is better.
//!
//! Quality precedes age in the sense that a stale high-quality value loses to
//! a fresh one (rule 2) and equal-rank simultaneous values are separated by
//! quality (rule 4); ordering by measurement time only applies once link and
//! freshness agree.
//!
//! Fusion never merges identities: association is the province of
//! [`crate::identity`] and the engine's association policy.

use crate::freshness::{FieldQuality, TimedField};
use crate::observation::TrafficLink;

#[cfg(test)]
mod tests;

/// Fusion precedence of a link class, higher wins between fresh values.
///
/// Transmissions from the target itself outrank ground-derived reports.
/// Mode S surveillance replies come from the target but carry neither
/// integrity indicators nor position, so they rank below the target's own
/// ADS-B. ADS-R relays one aircraft's own report and outranks TIS-B, which
/// is derived from ground surveillance of the target.
#[must_use]
pub fn link_rank(link: TrafficLink) -> u8 {
    match link {
        TrafficLink::Adsb1090 | TrafficLink::Uat978 => 3,
        TrafficLink::ModeS => 2,
        TrafficLink::AdsR { .. } => 1,
        TrafficLink::Tisb { .. } => 0,
    }
}

/// Outcome of offering one candidate value to a fused field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldUpdate {
    /// The candidate lost precedence; the field is unchanged.
    Rejected,
    /// The candidate won and carries a different value.
    Changed,
    /// The candidate won but carries the same value, so only freshness and
    /// provenance advanced. Callers suppress deltas for this outcome.
    Refreshed,
}

impl FieldUpdate {
    /// Whether the field's value differs from what a consumer last saw.
    #[must_use]
    pub fn is_meaningful(self) -> bool {
        matches!(self, Self::Changed)
    }
}

/// Offer `candidate` to a fused field, applying the precedence rules of this
/// module.
///
/// `fresh_window_micros` is the field's own freshness window: each field ages
/// independently, so a callsign update cannot make an old position fresh.
pub fn fuse<T: PartialEq>(
    slot: &mut Option<TimedField<T>>,
    candidate: TimedField<T>,
    fresh_window_micros: u64,
) -> FieldUpdate {
    let Some(current) = slot.as_ref() else {
        *slot = Some(candidate);
        return FieldUpdate::Changed;
    };

    if !accepts_replacement(current, &candidate, fresh_window_micros) {
        return FieldUpdate::Rejected;
    }

    let changed = current.value != candidate.value;
    *slot = Some(candidate);
    if changed {
        FieldUpdate::Changed
    } else {
        FieldUpdate::Refreshed
    }
}

fn accepts_replacement<T>(
    current: &TimedField<T>,
    candidate: &TimedField<T>,
    fresh_window_micros: u64,
) -> bool {
    if candidate.observed_at_micros < current.observed_at_micros {
        return false;
    }

    if !current.is_fresh(candidate.observed_at_micros, fresh_window_micros) {
        return true;
    }

    let current_rank = link_rank(current.provenance.link);
    let candidate_rank = link_rank(candidate.provenance.link);
    if candidate_rank != current_rank {
        return candidate_rank > current_rank;
    }

    if candidate.observed_at_micros > current.observed_at_micros {
        return true;
    }

    is_better_quality(candidate.quality, current.quality)
}

/// Whether `candidate` dominates `current`: no indicator is worse and at
/// least one is better.
///
/// A present indicator is better than an absent one, because absence means
/// the link never reported it rather than reporting a low value.
fn is_better_quality(candidate: FieldQuality, current: FieldQuality) -> bool {
    let pairs = [
        (candidate.nic, current.nic),
        (candidate.nac_p, current.nac_p),
        (candidate.nac_v, current.nac_v),
        (candidate.sil, current.sil),
    ];

    let mut any_better = false;
    for (candidate_indicator, current_indicator) in pairs {
        match (candidate_indicator, current_indicator) {
            (Some(candidate_value), Some(current_value)) => {
                if candidate_value < current_value {
                    return false;
                }
                any_better |= candidate_value > current_value;
            }
            (Some(_), None) => any_better = true,
            (None, Some(_)) => return false,
            (None, None) => {}
        }
    }
    any_better
}

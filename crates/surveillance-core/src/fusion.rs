//! Cross-source field fusion policy.
//!
//! Fusion decides which observation's value a fused field keeps when the same
//! target is reported by several sources. The decision procedure is per field
//! and evaluates, in order:
//!
//! 1. A value whose observation time was never established cannot displace
//!    one that is both established and still fresh. A provider that will not
//!    say when it measured has said nothing about age, and must not be read
//!    as if it had measured on arrival.
//! 2. The converse also holds: an established time is never blocked by an
//!    unestablished one. An arrival stamp is not a measurement time, so it
//!    cannot act as an age floor — otherwise a source reporting faster than a
//!    trustworthy source's mapping latency would hold a field against it
//!    forever, and which value survived would depend on which arrived first.
//! 3. Between two comparably established times, a strictly older observation
//!    never replaces a newer applied value, so a delayed report — a late
//!    network message especially — cannot undo a current one.
//! 4. A value whose age exceeds its freshness window is stale, and any
//!    candidate replaces it. A fresh ground-derived report therefore beats a
//!    stale direct one.
//! 5. Between fresh values, the higher [`origin_rank`] wins: what produced
//!    the observation decides, never how it was delivered.
//! 6. At equal rank, the newer observation wins; two observations of the same
//!    instant are resolved by quality, where better means no indicator is
//!    worse and at least one is better.
//!
//! Declared quality deliberately does not override origin. A source's own
//! integrity indicators are exactly what a poor source would overstate, so
//! they separate peers rather than promote a weaker origin above a stronger
//! one.
//!
//! Age is always the pessimistic end of any timing uncertainty, so a loosely
//! mapped provider time is treated as older than a tightly mapped one rather
//! than as equally current.
//!
//! Delivery path never appears in this procedure. A report is not more
//! authoritative for having arrived over a socket rather than a radio, nor
//! less; only its origin, its declared quality, and its trustworthy age
//! matter. Fusion also never merges identities: association is the province
//! of [`crate::identity`] and the engine's association policy.

use crate::freshness::{FieldQuality, TimedField};
use crate::observation::ObservationOrigin;

#[cfg(test)]
mod tests;

/// Fusion precedence of an origin, higher wins between fresh values.
///
/// The target's own transmissions outrank anything derived about it. Among
/// those, its ADS-B carries position and integrity while a Mode S reply
/// carries neither, and a rebroadcast is one relay removed from the original.
/// Ground-derived estimates rank below all of them, and a fused product whose
/// inputs were not itemized ranks below a named sensor because its weakest
/// contributor cannot be identified.
#[must_use]
pub fn origin_rank(origin: ObservationOrigin) -> u8 {
    match origin {
        ObservationOrigin::AdsbDirect { .. } => 9,
        ObservationOrigin::ModeSReply => 8,
        ObservationOrigin::AdsR { .. } => 7,
        ObservationOrigin::Mlat => 6,
        ObservationOrigin::Radar => 5,
        ObservationOrigin::Tisb { .. } => 4,
        ObservationOrigin::PanelFused => 3,
        ObservationOrigin::ProviderFused => 2,
        ObservationOrigin::Replay => 1,
        ObservationOrigin::Unknown => 0,
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
    /// Whether the candidate changed value, time, quality, or provenance.
    #[must_use]
    pub(crate) fn was_applied(self) -> bool {
        !matches!(self, Self::Rejected)
    }

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
    // Age is evaluated at the later of the two ingress times, which is the
    // latest moment the engine knows to have happened.
    let now_micros = candidate
        .time
        .received_at_micros
        .max(current.time.received_at_micros);
    let current_is_fresh = current.is_fresh(now_micros, fresh_window_micros);
    let candidate_established = candidate.time.is_established();
    let current_established = current.time.is_established();

    if !candidate_established && current_established && current_is_fresh {
        return false;
    }

    // An established time is better evidence than an unestablished one, so it
    // is never blocked by it. Without this the ingress stamp of a source that
    // would not say when it measured acts as an age floor, and a source
    // reporting faster than a trustworthy one's mapping latency holds the
    // field against it indefinitely — the exact inversion this policy exists
    // to prevent.
    if candidate_established && !current_established {
        return true;
    }

    // Comparing measurement times is only meaningful when both were
    // established the same way; otherwise one of them is an arrival stamp
    // wearing a measurement's name.
    if candidate.observed_at_micros() < current.observed_at_micros() {
        return false;
    }

    if !current_is_fresh {
        return true;
    }

    let current_rank = origin_rank(current.provenance.origin);
    let candidate_rank = origin_rank(candidate.provenance.origin);
    if candidate_rank != current_rank {
        return candidate_rank > current_rank;
    }

    if candidate.observed_at_micros() > current.observed_at_micros() {
        return true;
    }

    is_better_quality(candidate.quality, current.quality)
}

/// Whether `candidate` dominates `current`: no indicator is worse and at
/// least one is better.
///
/// A present indicator is better than an absent one, because absence means
/// the source never reported it rather than reporting a low value.
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

//! Deciding when two tracks describe one aircraft.
//!
//! ICAO-qualified identities need no decision here: direct ADS-B, ADS-R, and
//! TIS-B carrying an ICAO address normalize to one [`crate::TrackKey`], so
//! they reach one track through the engine's key index and per-field
//! precedence picks the winning link. This module covers the rows of the
//! association table that address equality cannot settle — a ground-assigned
//! track file or an anonymized self-assigned address against a cooperative
//! identity — where the only available evidence is kinematic.
//!
//! Evidence is deliberately slow to accumulate and slow to discard. A single
//! coincidental alignment must not merge two aircraft, and a single outlier
//! must not split one, so association requires several consecutive agreeing
//! observations and dissociation requires several consecutive disagreements.
//! Position proximity alone is never sufficient: velocity and altitude must
//! agree too, and callsign equality is not evidence at all.

use alloc::vec::Vec;

use crate::freshness::{FreshnessPolicy, TimedField};
use crate::geodesy::great_circle_distance_m;
use crate::track::{AddressNamespace, TrackId, TrackSnapshot};

#[cfg(test)]
mod tests;

/// Thresholds and hysteresis governing kinematic correlation.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct CorrelationPolicy {
    /// Largest horizontal separation two reports of one aircraft may show.
    pub max_position_separation_m: f64,
    /// Largest barometric altitude difference between agreeing reports.
    pub max_altitude_difference_ft: i32,
    /// Largest ground-speed difference between agreeing reports.
    pub max_ground_speed_difference_kt: f64,
    /// Consecutive agreements required before two tracks associate.
    pub associate_after: u8,
    /// Consecutive disagreements required before an association is dropped.
    pub dissociate_after: u8,
    /// Silence on the primary identity, while another associated identity
    /// keeps reporting, after which the track demotes to the live identity.
    pub demote_after_micros: u64,
}

impl Default for CorrelationPolicy {
    fn default() -> Self {
        Self {
            max_position_separation_m: 500.0,
            max_altitude_difference_ft: 200,
            max_ground_speed_difference_kt: 50.0,
            associate_after: 4,
            dissociate_after: 3,
            demote_after_micros: 20_000_000,
        }
    }
}

/// Running evidence about one candidate peer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CorrelationEvidence {
    pub(crate) peer: TrackId,
    agreements: u8,
    disagreements: u8,
}

impl CorrelationEvidence {
    fn new(peer: TrackId) -> Self {
        Self {
            peer,
            agreements: 0,
            disagreements: 0,
        }
    }

    /// Fold one comparison into the running counts. Agreement and
    /// disagreement each reset the other, so only consecutive runs count.
    fn observe(&mut self, agrees: bool) {
        if agrees {
            self.agreements = self.agreements.saturating_add(1);
            self.disagreements = 0;
        } else {
            self.disagreements = self.disagreements.saturating_add(1);
            self.agreements = 0;
        }
    }
}

/// Whether two namespaces may ever describe one aircraft.
///
/// Surface vehicles never correlate with aircraft, and an unclassified
/// non-ICAO ADS-B address is ineligible for the same reason: it may yet turn
/// out to be a vehicle, and a merge decided before its emitter category
/// arrives could not be justified. Two identities in the same namespace are
/// already separated by address, so equality — not correlation — is what
/// would join them. Everything else is decided kinematically, with at least
/// one side being an identity that cannot be trusted as globally unique.
#[must_use]
pub fn may_correlate(left: AddressNamespace, right: AddressNamespace) -> bool {
    if !is_correlatable(left) || !is_correlatable(right) || left == right {
        return false;
    }
    (is_uncertain(left) && (is_uncertain(right) || is_cooperative(right)))
        || (is_cooperative(left) && is_uncertain(right))
}

/// Whether a namespace may take part in correlation at all.
fn is_correlatable(namespace: AddressNamespace) -> bool {
    !matches!(
        namespace,
        AddressNamespace::SurfaceVehicle | AddressNamespace::AdsbNonIcao
    )
}

/// Whether a namespace names an identity that cannot be trusted as globally
/// unique, which is what makes kinematic evidence necessary.
fn is_uncertain(namespace: AddressNamespace) -> bool {
    matches!(
        namespace,
        AddressNamespace::TisbTrackFile
            | AddressNamespace::TisbModeATrackFile
            | AddressNamespace::SelfAssigned
    )
}

/// Whether a namespace names a globally unique cooperative address.
fn is_cooperative(namespace: AddressNamespace) -> bool {
    matches!(
        namespace,
        AddressNamespace::Icao | AddressNamespace::FixedBeacon
    )
}

/// Authority of a namespace when choosing a merged track's primary identity.
///
/// A globally unique cooperative address outranks a ground-assigned track
/// file or an anonymized address, so a merged track presents the identity a
/// consumer can act on.
#[must_use]
pub fn namespace_rank(namespace: AddressNamespace) -> u8 {
    match namespace {
        AddressNamespace::Icao => 6,
        AddressNamespace::FixedBeacon => 5,
        AddressNamespace::SelfAssigned => 4,
        AddressNamespace::AdsbNonIcao => 3,
        AddressNamespace::TisbTrackFile => 2,
        AddressNamespace::TisbModeATrackFile => 1,
        AddressNamespace::SurfaceVehicle => 0,
    }
}

/// Whether two snapshots agree kinematically at `now_micros`.
///
/// Returns `None` when the pair cannot be judged — one side lacks a fresh
/// position — so an unobservable interval accumulates neither agreement nor
/// disagreement rather than being read as a mismatch.
#[must_use]
pub fn agrees(
    left: &TrackSnapshot,
    right: &TrackSnapshot,
    now_micros: u64,
    policy: &CorrelationPolicy,
    freshness: &FreshnessPolicy,
) -> Option<bool> {
    let left_position = fresh(
        left.position.as_ref(),
        now_micros,
        freshness.position_fresh_micros,
    )?;
    let right_position = fresh(
        right.position.as_ref(),
        now_micros,
        freshness.position_fresh_micros,
    )?;

    let separation_m = great_circle_distance_m(left_position, right_position);
    if separation_m > policy.max_position_separation_m {
        return Some(false);
    }

    Some(
        altitudes_agree(left, right, policy)
            && speeds_agree(left, right, now_micros, policy, freshness),
    )
}

/// Altitude disagreement is decisive; absence on either side is not, because
/// a report without altitude is not evidence of a different aircraft.
fn altitudes_agree(
    left: &TrackSnapshot,
    right: &TrackSnapshot,
    policy: &CorrelationPolicy,
) -> bool {
    let (Some(left_altitude), Some(right_altitude)) = (
        left.pressure_altitude_ft.as_ref(),
        right.pressure_altitude_ft.as_ref(),
    ) else {
        return true;
    };
    left_altitude.value.abs_diff(right_altitude.value)
        <= policy.max_altitude_difference_ft.unsigned_abs()
}

fn speeds_agree(
    left: &TrackSnapshot,
    right: &TrackSnapshot,
    now_micros: u64,
    policy: &CorrelationPolicy,
    freshness: &FreshnessPolicy,
) -> bool {
    let window = freshness.velocity_fresh_micros;
    let (Some(left_velocity), Some(right_velocity)) = (
        fresh(left.velocity.as_ref(), now_micros, window),
        fresh(right.velocity.as_ref(), now_micros, window),
    ) else {
        return true;
    };
    let (Some(left_speed), Some(right_speed)) = (
        left_velocity.ground_speed_kt,
        right_velocity.ground_speed_kt,
    ) else {
        return true;
    };
    (left_speed - right_speed).abs() <= policy.max_ground_speed_difference_kt
}

fn fresh<T: Copy>(field: Option<&TimedField<T>>, now_micros: u64, window_micros: u64) -> Option<T> {
    field
        .filter(|timed| timed.is_fresh(now_micros, window_micros))
        .map(|timed| timed.value)
}

/// Fold one round of comparisons into a track's evidence, reporting a peer
/// that has now accumulated enough agreement to associate.
///
/// Peers are held in [`TrackId`] order and a peer that reaches the
/// dissociation threshold is dropped, so evidence neither grows without bound
/// nor depends on observation order.
pub(crate) fn record(
    evidence: &mut Vec<CorrelationEvidence>,
    outcomes: &[(TrackId, bool)],
    policy: &CorrelationPolicy,
) -> Option<TrackId> {
    for (peer, agrees) in outcomes {
        let index = match evidence.binary_search_by_key(peer, |entry| entry.peer) {
            Ok(index) => index,
            Err(index) => {
                evidence.insert(index, CorrelationEvidence::new(*peer));
                index
            }
        };
        evidence[index].observe(*agrees);
    }

    evidence.retain(|entry| entry.disagreements < policy.dissociate_after);
    evidence
        .iter()
        .find(|entry| entry.agreements >= policy.associate_after)
        .map(|entry| entry.peer)
}

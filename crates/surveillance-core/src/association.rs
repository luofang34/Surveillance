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
    /// Largest gap between samples in one consecutive evidence run.
    pub max_evidence_gap_micros: u64,
    /// Largest span from the first to the last agreement in one evidence run.
    pub max_evidence_window_micros: u64,
    /// Largest number of candidate peers retained by one track.
    pub max_candidate_peers: usize,
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
            max_evidence_gap_micros: 5_000_000,
            max_evidence_window_micros: 15_000_000,
            max_candidate_peers: 64,
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
    run_started_at_micros: u64,
    last_observed_at_micros: Option<u64>,
}

impl CorrelationEvidence {
    fn new(peer: TrackId, at_micros: u64) -> Self {
        Self {
            peer,
            agreements: 0,
            disagreements: 0,
            run_started_at_micros: at_micros,
            last_observed_at_micros: None,
        }
    }

    /// Fold one comparison into the running counts. Agreement and
    /// disagreement each reset the other, so only consecutive runs count.
    fn observe(&mut self, agrees: bool, at_micros: u64, policy: &CorrelationPolicy) {
        if self
            .last_observed_at_micros
            .is_some_and(|last| at_micros <= last)
        {
            return;
        }
        if self.last_observed_at_micros.is_some_and(|last| {
            at_micros.saturating_sub(last) > policy.max_evidence_gap_micros
                || at_micros.saturating_sub(self.run_started_at_micros)
                    > policy.max_evidence_window_micros
        }) {
            self.agreements = 0;
            self.disagreements = 0;
            self.run_started_at_micros = at_micros;
        }
        if self.agreements == 0 && self.disagreements == 0 {
            self.run_started_at_micros = at_micros;
        }
        if agrees {
            self.agreements = self.agreements.saturating_add(1);
            self.disagreements = 0;
        } else {
            self.disagreements = self.disagreements.saturating_add(1);
            self.agreements = 0;
            self.run_started_at_micros = at_micros;
        }
        self.last_observed_at_micros = Some(at_micros);
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
    if separation_m > position_limit_m(left, right, policy) {
        return Some(false);
    }

    let altitude = altitudes_agree(left, right, now_micros, policy, freshness);
    let speed = speeds_agree(left, right, now_micros, policy, freshness);
    if altitude == Some(false) || speed == Some(false) {
        return Some(false);
    }
    (altitude == Some(true) && speed == Some(true)).then_some(true)
}

fn position_limit_m(
    left: &TrackSnapshot,
    right: &TrackSnapshot,
    policy: &CorrelationPolicy,
) -> f64 {
    let left_radius = left
        .position
        .as_ref()
        .and_then(|position| position.quality.nic)
        .and_then(nic_containment_radius_m);
    let right_radius = right
        .position
        .as_ref()
        .and_then(|position| position.quality.nic)
        .and_then(nic_containment_radius_m);
    left_radius
        .zip(right_radius)
        .map_or(policy.max_position_separation_m, |(left, right)| {
            (left + right).min(policy.max_position_separation_m)
        })
}

fn nic_containment_radius_m(nic: u8) -> Option<f64> {
    match nic {
        11 => Some(7.5),
        10 => Some(25.0),
        9 => Some(75.0),
        8 => Some(185.2),
        7 => Some(370.4),
        6 => Some(926.0),
        5 => Some(1_852.0),
        4 => Some(3_704.0),
        3 => Some(7_408.0),
        2 => Some(14_816.0),
        1 => Some(37_040.0),
        _ => None,
    }
}

fn altitudes_agree(
    left: &TrackSnapshot,
    right: &TrackSnapshot,
    now_micros: u64,
    policy: &CorrelationPolicy,
    freshness: &FreshnessPolicy,
) -> Option<bool> {
    let window = freshness.position_fresh_micros;
    let left_altitude = fresh(left.pressure_altitude_ft.as_ref(), now_micros, window)?;
    let right_altitude = fresh(right.pressure_altitude_ft.as_ref(), now_micros, window)?;
    Some(left_altitude.abs_diff(right_altitude) <= policy.max_altitude_difference_ft.unsigned_abs())
}

fn speeds_agree(
    left: &TrackSnapshot,
    right: &TrackSnapshot,
    now_micros: u64,
    policy: &CorrelationPolicy,
    freshness: &FreshnessPolicy,
) -> Option<bool> {
    let window = freshness.velocity_fresh_micros;
    let left_velocity = fresh(left.velocity.as_ref(), now_micros, window)?;
    let right_velocity = fresh(right.velocity.as_ref(), now_micros, window)?;
    let left_speed = left_velocity.ground_speed_kt?;
    let right_speed = right_velocity.ground_speed_kt?;
    Some((left_speed - right_speed).abs() <= policy.max_ground_speed_difference_kt)
}

fn fresh<T: Copy>(field: Option<&TimedField<T>>, now_micros: u64, window_micros: u64) -> Option<T> {
    field
        .filter(|timed| timed.is_fresh(now_micros, window_micros))
        .map(|timed| timed.value)
}

/// Fold one round of comparisons into a track's evidence, reporting a peer
/// that has now accumulated enough agreement to associate.
///
/// Peers are held in [`TrackId`] order. Old evidence expires before a new
/// sample is applied. The configured candidate bound limits per-track memory.
pub(crate) fn record(
    evidence: &mut Vec<CorrelationEvidence>,
    outcomes: &[(TrackId, bool)],
    at_micros: u64,
    policy: &CorrelationPolicy,
) -> Option<TrackId> {
    evidence.retain(|entry| {
        entry
            .last_observed_at_micros
            .is_some_and(|last| at_micros.saturating_sub(last) <= policy.max_evidence_window_micros)
    });

    for (peer, agrees) in outcomes {
        let index = match evidence.binary_search_by_key(peer, |entry| entry.peer) {
            Ok(index) => index,
            Err(_) => {
                if !make_candidate_slot(evidence, policy) {
                    continue;
                }
                let index = evidence
                    .binary_search_by_key(peer, |entry| entry.peer)
                    .unwrap_or_else(|index| index);
                evidence.insert(index, CorrelationEvidence::new(*peer, at_micros));
                index
            }
        };
        evidence[index].observe(*agrees, at_micros, policy);
    }

    evidence.retain(|entry| entry.disagreements < policy.dissociate_after);
    evidence
        .iter()
        .find(|entry| entry.agreements >= policy.associate_after)
        .map(|entry| entry.peer)
}

fn make_candidate_slot(
    evidence: &mut Vec<CorrelationEvidence>,
    policy: &CorrelationPolicy,
) -> bool {
    if policy.max_candidate_peers == 0 {
        return false;
    }
    if evidence.len() < policy.max_candidate_peers {
        return true;
    }
    let Some((oldest, _)) = evidence
        .iter()
        .enumerate()
        .min_by_key(|(_, entry)| (entry.last_observed_at_micros, entry.peer))
    else {
        return false;
    };
    evidence.remove(oldest);
    true
}

/// Remove all evidence that names `peer`.
pub(crate) fn forget_peer(evidence: &mut Vec<CorrelationEvidence>, peer: TrackId) {
    evidence.retain(|entry| entry.peer != peer);
}

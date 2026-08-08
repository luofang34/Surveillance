#![allow(clippy::expect_used, clippy::panic)]

use alloc::vec::Vec;

use crate::source::SourceRef;

use crate::association::{
    CorrelationEvidence, CorrelationPolicy, agrees, may_correlate, namespace_rank, record,
};
use crate::freshness::{FieldProvenance, FieldQuality, FreshnessPolicy, TimedField};
use crate::observation::{TrafficLink, VelocityObservation, Wgs84Position};
use crate::track::{AddressNamespace, TrackId, TrackKey, TrackSnapshot};

fn snapshot(
    namespace: AddressNamespace,
    at: u64,
    latitude_deg: f64,
    longitude_deg: f64,
) -> TrackSnapshot {
    let key = TrackKey::new(namespace, 0x0000_0001);
    let mut snapshot = TrackSnapshot::new(TrackId::new(1), key, at);
    let provenance = FieldProvenance::new(TrafficLink::Adsb1090, namespace, SourceRef::default());
    snapshot.position = Some(TimedField::new(
        Wgs84Position {
            latitude_deg,
            longitude_deg,
        },
        at,
        FieldQuality::default(),
        provenance,
    ));
    snapshot
}

fn with_altitude(mut snapshot: TrackSnapshot, at: u64, feet: i32) -> TrackSnapshot {
    let provenance = FieldProvenance::new(
        TrafficLink::Adsb1090,
        snapshot.key.namespace,
        SourceRef::default(),
    );
    snapshot.pressure_altitude_ft = Some(TimedField::new(
        feet,
        at,
        FieldQuality::default(),
        provenance,
    ));
    snapshot
}

fn with_speed(mut snapshot: TrackSnapshot, at: u64, knots: f64) -> TrackSnapshot {
    let provenance = FieldProvenance::new(
        TrafficLink::Adsb1090,
        snapshot.key.namespace,
        SourceRef::default(),
    );
    snapshot.velocity = Some(TimedField::new(
        VelocityObservation {
            ground_speed_kt: Some(knots),
            ..VelocityObservation::default()
        },
        at,
        FieldQuality::default(),
        provenance,
    ));
    snapshot
}

#[test]
fn a_vehicle_never_correlates_with_an_aircraft() {
    use AddressNamespace::{Icao, SelfAssigned, SurfaceVehicle, TisbTrackFile};
    for other in [Icao, SelfAssigned, TisbTrackFile, SurfaceVehicle] {
        assert!(!may_correlate(SurfaceVehicle, other));
        assert!(!may_correlate(other, SurfaceVehicle));
    }
}

#[test]
fn identities_in_one_namespace_are_separated_by_address_not_correlation() {
    use AddressNamespace::{Icao, SelfAssigned, TisbTrackFile};
    assert!(!may_correlate(Icao, Icao));
    assert!(!may_correlate(TisbTrackFile, TisbTrackFile));
    assert!(!may_correlate(SelfAssigned, SelfAssigned));
}

#[test]
fn uncertain_identities_correlate_with_cooperative_ones() {
    use AddressNamespace::{Icao, SelfAssigned, TisbTrackFile};
    assert!(may_correlate(TisbTrackFile, Icao));
    assert!(may_correlate(Icao, TisbTrackFile));
    assert!(may_correlate(SelfAssigned, Icao));
    assert!(may_correlate(TisbTrackFile, SelfAssigned));
}

#[test]
fn a_cooperative_address_outranks_a_ground_assigned_one() {
    use AddressNamespace::{FixedBeacon, Icao, SelfAssigned, SurfaceVehicle, TisbTrackFile};
    assert!(namespace_rank(Icao) > namespace_rank(FixedBeacon));
    assert!(namespace_rank(FixedBeacon) > namespace_rank(SelfAssigned));
    assert!(namespace_rank(SelfAssigned) > namespace_rank(TisbTrackFile));
    assert!(namespace_rank(TisbTrackFile) > namespace_rank(SurfaceVehicle));
}

#[test]
fn co_located_reports_with_matching_state_agree() {
    let policy = CorrelationPolicy::default();
    let freshness = FreshnessPolicy::default();
    let left = with_speed(
        with_altitude(
            snapshot(AddressNamespace::Icao, 1_000, 40.0, -74.0),
            1_000,
            10_000,
        ),
        1_000,
        250.0,
    );
    let right = with_speed(
        with_altitude(
            snapshot(AddressNamespace::TisbTrackFile, 1_000, 40.001, -74.001),
            1_000,
            10_050,
        ),
        1_000,
        260.0,
    );

    assert_eq!(
        agrees(&left, &right, 1_000, &policy, &freshness),
        Some(true)
    );
}

#[test]
fn separated_reports_disagree() {
    let policy = CorrelationPolicy::default();
    let freshness = FreshnessPolicy::default();
    let left = snapshot(AddressNamespace::Icao, 1_000, 40.0, -74.0);
    let right = snapshot(AddressNamespace::TisbTrackFile, 1_000, 40.1, -74.0);

    assert_eq!(
        agrees(&left, &right, 1_000, &policy, &freshness),
        Some(false)
    );
}

#[test]
fn co_located_reports_at_different_altitudes_disagree() {
    let policy = CorrelationPolicy::default();
    let freshness = FreshnessPolicy::default();
    let left = with_altitude(
        snapshot(AddressNamespace::Icao, 1_000, 40.0, -74.0),
        1_000,
        10_000,
    );
    let right = with_altitude(
        snapshot(AddressNamespace::TisbTrackFile, 1_000, 40.0005, -74.0),
        1_000,
        30_000,
    );

    assert_eq!(
        agrees(&left, &right, 1_000, &policy, &freshness),
        Some(false)
    );
}

#[test]
fn co_located_reports_at_different_speeds_disagree() {
    let policy = CorrelationPolicy::default();
    let freshness = FreshnessPolicy::default();
    let left = with_speed(
        snapshot(AddressNamespace::Icao, 1_000, 40.0, -74.0),
        1_000,
        120.0,
    );
    let right = with_speed(
        snapshot(AddressNamespace::TisbTrackFile, 1_000, 40.0005, -74.0),
        1_000,
        450.0,
    );

    assert_eq!(
        agrees(&left, &right, 1_000, &policy, &freshness),
        Some(false)
    );
}

#[test]
fn a_pair_that_cannot_be_judged_yields_no_evidence() {
    let policy = CorrelationPolicy::default();
    let freshness = FreshnessPolicy::default();
    let positioned = snapshot(AddressNamespace::Icao, 1_000, 40.0, -74.0);
    let mut unpositioned = snapshot(AddressNamespace::TisbTrackFile, 1_000, 40.0, -74.0);
    unpositioned.position = None;

    assert_eq!(
        agrees(&positioned, &unpositioned, 1_000, &policy, &freshness),
        None
    );

    let stale_at = 1_000 + freshness.position_fresh_micros + 1;
    assert_eq!(
        agrees(&positioned, &positioned, stale_at, &policy, &freshness),
        None,
        "a stale position is not evidence either way"
    );
}

#[test]
fn one_coincidental_alignment_does_not_associate() {
    let policy = CorrelationPolicy::default();
    let mut evidence: Vec<CorrelationEvidence> = Vec::new();
    let peer = TrackId::new(2);

    for round in 1..policy.associate_after {
        assert_eq!(
            record(&mut evidence, &[(peer, true)], &policy),
            None,
            "round {round} must not be enough to associate"
        );
    }
    assert_eq!(record(&mut evidence, &[(peer, true)], &policy), Some(peer));
}

#[test]
fn a_single_outlier_does_not_break_an_accumulating_run() {
    let policy = CorrelationPolicy::default();
    let mut evidence: Vec<CorrelationEvidence> = Vec::new();
    let peer = TrackId::new(2);

    for _ in 0..policy.associate_after - 1 {
        record(&mut evidence, &[(peer, true)], &policy);
    }
    record(&mut evidence, &[(peer, false)], &policy);
    assert!(
        !evidence.is_empty(),
        "one disagreement must not discard the candidate"
    );

    for _ in 0..policy.associate_after - 1 {
        assert_eq!(record(&mut evidence, &[(peer, true)], &policy), None);
    }
    assert_eq!(
        record(&mut evidence, &[(peer, true)], &policy),
        Some(peer),
        "a fresh consecutive run associates"
    );
}

#[test]
fn sustained_disagreement_discards_a_candidate() {
    let policy = CorrelationPolicy::default();
    let mut evidence: Vec<CorrelationEvidence> = Vec::new();
    let peer = TrackId::new(2);

    for _ in 0..policy.dissociate_after {
        record(&mut evidence, &[(peer, false)], &policy);
    }
    assert!(
        evidence.is_empty(),
        "a candidate that keeps disagreeing is dropped"
    );
}

#[test]
fn evidence_is_held_in_track_identity_order() {
    let policy = CorrelationPolicy::default();
    let mut evidence: Vec<CorrelationEvidence> = Vec::new();
    for peer in [TrackId::new(9), TrackId::new(3), TrackId::new(5)] {
        record(&mut evidence, &[(peer, true)], &policy);
    }

    let peers: Vec<u64> = evidence.iter().map(|entry| entry.peer.get()).collect();
    assert_eq!(peers, [3, 5, 9]);
}

#![allow(clippy::expect_used, clippy::panic)]

use crate::freshness::{FieldProvenance, FieldQuality, FreshnessPolicy, TimedField};
use crate::observation::{ObservationOrigin, ObservationTime, VelocityObservation, Wgs84Position};
use crate::projection::{ProjectionBasis, project_position};
use crate::source::{Band, SourceRef};
use crate::track::{AddressNamespace, TrackId, TrackKey, TrackPhase, TrackSnapshot};

const ICAO: TrackKey = TrackKey {
    namespace: AddressNamespace::Icao,
    address: 0x00AB_CD01,
};

const START: Wgs84Position = Wgs84Position {
    latitude_deg: 47.6,
    longitude_deg: -122.3,
};

fn provenance() -> FieldProvenance {
    FieldProvenance::new(
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
        ICAO.namespace,
        SourceRef::default(),
    )
}

fn snapshot(position_at: u64) -> TrackSnapshot {
    let mut snapshot = TrackSnapshot::new(TrackId::new(1), ICAO, position_at);
    snapshot.position = Some(TimedField::new(
        START,
        ObservationTime::local(position_at),
        FieldQuality::default(),
        provenance(),
    ));
    snapshot.last_observed_at_micros = position_at;
    snapshot
}

fn with_velocity(
    mut snapshot: TrackSnapshot,
    at: u64,
    ground_speed_kt: Option<f64>,
    track_angle_deg_true: Option<f64>,
) -> TrackSnapshot {
    snapshot.velocity = Some(TimedField::new(
        VelocityObservation {
            ground_speed_kt,
            track_angle_deg_true,
            ..VelocityObservation::default()
        },
        ObservationTime::local(at),
        FieldQuality::default(),
        provenance(),
    ));
    snapshot
}

fn moving_snapshot(at: u64) -> TrackSnapshot {
    with_velocity(snapshot(at), at, Some(120.0), Some(0.0))
}

#[test]
fn a_track_without_a_position_has_no_projection() {
    let snapshot = TrackSnapshot::new(TrackId::new(1), ICAO, 0);
    assert_eq!(
        project_position(&snapshot, 0, &FreshnessPolicy::default()),
        None
    );
}

#[test]
fn a_just_reported_position_is_reported() {
    let snapshot = moving_snapshot(1_000_000);
    let projection = project_position(&snapshot, 1_000_000, &FreshnessPolicy::default())
        .expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
    assert_eq!(projection.position, START);
}

#[test]
fn a_fresh_track_moves_along_its_reported_velocity() {
    let snapshot = moving_snapshot(1_000_000);
    // Ten seconds at 120 knots due north is one third of a nautical mile.
    let projection = project_position(&snapshot, 11_000_000, &FreshnessPolicy::default())
        .expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
    assert!(
        (projection.position.latitude_deg - (START.latitude_deg + 0.005_552)).abs() < 0.000_1,
        "latitude advanced to {}",
        projection.position.latitude_deg
    );
    assert!(
        (projection.position.longitude_deg - START.longitude_deg).abs() < 1e-9,
        "longitude moved to {}",
        projection.position.longitude_deg
    );
}

#[test]
fn projection_stops_at_the_projection_window() {
    let freshness = FreshnessPolicy {
        extrapolate_max_micros: 5_000_000,
        ..FreshnessPolicy::default()
    };
    let snapshot = moving_snapshot(1_000_000);
    let projection =
        project_position(&snapshot, 6_500_000, &freshness).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
    assert_eq!(projection.position, START);
}

#[test]
fn a_coasting_track_is_not_projected() {
    let mut snapshot = moving_snapshot(1_000_000);
    snapshot.phase = TrackPhase::Coasting;
    let projection = project_position(&snapshot, 5_000_000, &FreshnessPolicy::default())
        .expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
}

#[test]
fn a_silent_track_is_not_projected_even_before_advance_time() {
    // Without advance_time the phase still reads Active, but the silence has
    // passed the coast window, so projection must already have stopped.
    let freshness = FreshnessPolicy {
        coast_after_micros: 3_000_000,
        ..FreshnessPolicy::default()
    };
    let snapshot = moving_snapshot(1_000_000);
    let projection =
        project_position(&snapshot, 4_500_000, &freshness).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
}

#[test]
fn a_track_with_other_receptions_projects_from_its_older_position() {
    let mut snapshot = moving_snapshot(1_000_000);
    // A callsign report keeps the track alive but cannot refresh the position.
    snapshot.last_observed_at_micros = 9_000_000;
    let projection = project_position(&snapshot, 9_000_000, &FreshnessPolicy::default())
        .expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
}

#[test]
fn a_stale_velocity_is_not_projected() {
    let freshness = FreshnessPolicy {
        velocity_fresh_micros: 2_000_000,
        ..FreshnessPolicy::default()
    };
    let snapshot = moving_snapshot(1_000_000);
    let projection =
        project_position(&snapshot, 4_000_000, &freshness).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
}

#[test]
fn a_velocity_without_ground_speed_or_track_angle_is_not_projected() {
    let no_speed = with_velocity(snapshot(1_000_000), 1_000_000, None, Some(90.0));
    let projection = project_position(&no_speed, 3_000_000, &FreshnessPolicy::default())
        .expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);

    let no_angle = with_velocity(snapshot(1_000_000), 1_000_000, Some(120.0), None);
    let projection = project_position(&no_angle, 3_000_000, &FreshnessPolicy::default())
        .expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
}

#[test]
fn a_query_before_the_report_is_reported() {
    let snapshot = moving_snapshot(5_000_000);
    let projection = project_position(&snapshot, 1_000_000, &FreshnessPolicy::default())
        .expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
    assert_eq!(projection.position, START);
}

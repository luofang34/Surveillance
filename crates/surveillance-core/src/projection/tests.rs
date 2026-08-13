#![allow(clippy::expect_used, clippy::panic)]

use crate::freshness::{FieldProvenance, FieldQuality, FreshnessPolicy, TimedField};
use crate::observation::{
    ObservationOrigin, ObservationTime, VelocityObservation, VerticalRateSource, Wgs84Position,
};
use crate::projection::{ProjectionBasis, ProjectionBounds, project_position};
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

fn timed<T>(value: T, at: u64) -> TimedField<T> {
    TimedField::new(
        value,
        ObservationTime::local(at),
        FieldQuality::default(),
        provenance(),
    )
}

fn snapshot(position_at: u64) -> TrackSnapshot {
    let mut snapshot = TrackSnapshot::new(TrackId::new(1), ICAO, position_at);
    snapshot.position = Some(timed(START, position_at));
    snapshot.last_observed_at_micros = position_at;
    snapshot.projection_bounds =
        Some(ProjectionBounds::from_freshness(&FreshnessPolicy::default()));
    snapshot
}

fn with_bounds(mut snapshot: TrackSnapshot, bounds: ProjectionBounds) -> TrackSnapshot {
    snapshot.projection_bounds = Some(bounds);
    snapshot
}

fn with_velocity(
    mut snapshot: TrackSnapshot,
    at: u64,
    ground_speed_kt: Option<f64>,
    track_angle_deg_true: Option<f64>,
) -> TrackSnapshot {
    snapshot.velocity = Some(timed(
        VelocityObservation {
            ground_speed_kt,
            track_angle_deg_true,
            ..VelocityObservation::default()
        },
        at,
    ));
    snapshot
}

fn with_altitude(mut snapshot: TrackSnapshot, at: u64, feet: i32) -> TrackSnapshot {
    snapshot.pressure_altitude_ft = Some(timed(feet, at));
    snapshot
}

fn with_vertical_rate(
    mut snapshot: TrackSnapshot,
    rate_fpm: i32,
    source: Option<VerticalRateSource>,
) -> TrackSnapshot {
    let velocity = snapshot
        .velocity
        .as_mut()
        .expect("the test snapshot has a velocity");
    velocity.value.vertical_rate_fpm = Some(rate_fpm);
    velocity.value.vertical_rate_source = source;
    snapshot
}

fn moving_snapshot(at: u64) -> TrackSnapshot {
    with_velocity(snapshot(at), at, Some(120.0), Some(0.0))
}

#[test]
fn a_track_without_a_position_has_no_projection() {
    let snapshot = TrackSnapshot::new(TrackId::new(1), ICAO, 0);
    assert_eq!(project_position(&snapshot, 0), None);
}

#[test]
fn a_just_reported_position_is_reported() {
    let snapshot = moving_snapshot(1_000_000);
    let projection = project_position(&snapshot, 1_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
    assert_eq!(projection.position, START);
}

#[test]
fn a_fresh_track_moves_along_its_reported_velocity() {
    let snapshot = moving_snapshot(1_000_000);
    // Ten seconds at 120 knots due north is one third of a nautical mile.
    let projection = project_position(&snapshot, 11_000_000).expect("a positioned track projects");
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
fn a_snapshot_without_bounds_is_reported() {
    let mut snapshot = moving_snapshot(1_000_000);
    snapshot.projection_bounds = None;
    let projection = project_position(&snapshot, 11_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
    assert_eq!(projection.position, START);
}

#[test]
fn projection_stops_at_the_projection_window() {
    let bounds = ProjectionBounds {
        extrapolate_max_micros: 5_000_000,
        ..ProjectionBounds::from_freshness(&FreshnessPolicy::default())
    };
    let snapshot = with_bounds(moving_snapshot(1_000_000), bounds);
    let projection = project_position(&snapshot, 6_500_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
    assert_eq!(projection.position, START);
}

#[test]
fn a_fast_target_stops_at_the_distance_limit() {
    // Fifteen seconds at 240 knots is a full nautical mile, but the distance
    // limit of one kilometre cuts the leg short.
    let snapshot = with_velocity(snapshot(1_000_000), 1_000_000, Some(240.0), Some(0.0));
    let projection = project_position(&snapshot, 16_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
    assert!(
        (projection.position.latitude_deg - (START.latitude_deg + 0.008_994)).abs() < 0.000_1,
        "latitude advanced to {}",
        projection.position.latitude_deg
    );
}

#[test]
fn a_slow_target_never_reaches_the_distance_limit() {
    // Fifteen seconds at 60 knots is 463 metres, inside the one-kilometre
    // limit, so the time window alone bounds the leg.
    let snapshot = with_velocity(snapshot(1_000_000), 1_000_000, Some(60.0), Some(0.0));
    let projection = project_position(&snapshot, 16_000_000).expect("a positioned track projects");
    assert!(
        (projection.position.latitude_deg - (START.latitude_deg + 0.004_166)).abs() < 0.000_1,
        "latitude advanced to {}",
        projection.position.latitude_deg
    );
}

#[test]
fn a_coasting_track_is_not_projected() {
    let mut snapshot = moving_snapshot(1_000_000);
    snapshot.phase = TrackPhase::Coasting;
    let projection = project_position(&snapshot, 5_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
}

#[test]
fn a_silent_track_is_not_projected_even_before_advance_time() {
    // Without advance_time the phase still reads Active, but the silence has
    // passed the coast window, so projection must already have stopped.
    let bounds = ProjectionBounds {
        coast_after_micros: 3_000_000,
        ..ProjectionBounds::from_freshness(&FreshnessPolicy::default())
    };
    let snapshot = with_bounds(moving_snapshot(1_000_000), bounds);
    let projection = project_position(&snapshot, 4_500_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
}

#[test]
fn a_track_with_other_receptions_projects_from_its_older_position() {
    let mut snapshot = moving_snapshot(1_000_000);
    // A callsign report keeps the track alive but cannot refresh the position.
    snapshot.last_observed_at_micros = 9_000_000;
    let projection = project_position(&snapshot, 9_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
}

#[test]
fn a_stale_velocity_is_not_projected() {
    let bounds = ProjectionBounds {
        velocity_fresh_micros: 2_000_000,
        ..ProjectionBounds::from_freshness(&FreshnessPolicy::default())
    };
    let snapshot = with_bounds(moving_snapshot(1_000_000), bounds);
    let projection = project_position(&snapshot, 4_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
}

#[test]
fn a_velocity_without_ground_speed_or_track_angle_is_not_projected() {
    let no_speed = with_velocity(snapshot(1_000_000), 1_000_000, None, Some(90.0));
    let projection = project_position(&no_speed, 3_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);

    let no_angle = with_velocity(snapshot(1_000_000), 1_000_000, Some(120.0), None);
    let projection = project_position(&no_angle, 3_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
}

#[test]
fn a_query_before_the_report_is_reported() {
    let snapshot = moving_snapshot(5_000_000);
    let projection = project_position(&snapshot, 1_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
    assert_eq!(projection.position, START);
}

#[test]
fn projection_window_edges_match_the_freshness_inequalities() {
    let bounds = ProjectionBounds {
        extrapolate_max_micros: 5_000_000,
        coast_after_micros: 4_000_000,
        ..ProjectionBounds::from_freshness(&FreshnessPolicy::default())
    };
    let mut snapshot = with_bounds(moving_snapshot(1_000_000), bounds);

    // Age exactly at the window still projects: the window check is strict.
    // A newer callsign report keeps the silence inside the coast window.
    snapshot.last_observed_at_micros = 6_000_000;
    let projection = project_position(&snapshot, 6_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);

    // Silence exactly at the coast window still projects, matching the
    // strict inequality of the coasting transition itself.
    snapshot.last_observed_at_micros = 1_000_000;
    let projection = project_position(&snapshot, 5_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
}

#[test]
fn a_stationary_track_projects_to_its_reported_position() {
    let snapshot = with_velocity(snapshot(1_000_000), 1_000_000, Some(0.0), Some(270.0));
    let projection = project_position(&snapshot, 4_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
    assert!(
        (projection.position.latitude_deg - START.latitude_deg).abs() < 1e-9
            && (projection.position.longitude_deg - START.longitude_deg).abs() < 1e-9,
        "a stationary track moved to {:?}",
        projection.position
    );
}

#[test]
fn a_reported_projection_carries_the_reported_altitude() {
    let snapshot = with_altitude(moving_snapshot(1_000_000), 1_000_000, 4_500);
    let projection = project_position(&snapshot, 1_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Reported);
    assert_eq!(projection.pressure_altitude_ft, Some(4_500));
}

#[test]
fn an_extrapolated_projection_advances_altitude_by_the_vertical_rate() {
    // Ten seconds at 1 200 feet per minute climbs 200 feet.
    let snapshot = with_vertical_rate(
        with_altitude(moving_snapshot(1_000_000), 1_000_000, 4_500),
        1_200,
        Some(VerticalRateSource::Barometric),
    );
    let projection = project_position(&snapshot, 11_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
    assert_eq!(projection.pressure_altitude_ft, Some(4_700));
}

#[test]
fn a_descending_rate_moves_the_altitude_down() {
    let snapshot = with_vertical_rate(
        with_altitude(moving_snapshot(1_000_000), 1_000_000, 4_500),
        -600,
        None,
    );
    let projection = project_position(&snapshot, 16_000_000).expect("a positioned track projects");
    assert_eq!(projection.pressure_altitude_ft, Some(4_350));
}

#[test]
fn a_geometric_rate_does_not_move_a_pressure_altitude() {
    let snapshot = with_vertical_rate(
        with_altitude(moving_snapshot(1_000_000), 1_000_000, 4_500),
        1_200,
        Some(VerticalRateSource::Geometric),
    );
    let projection = project_position(&snapshot, 11_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
    assert_eq!(projection.pressure_altitude_ft, Some(4_500));
}

#[test]
fn an_extrapolated_projection_without_a_rate_keeps_the_reported_altitude() {
    let snapshot = with_altitude(moving_snapshot(1_000_000), 1_000_000, 4_500);
    let projection = project_position(&snapshot, 11_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
    assert_eq!(projection.pressure_altitude_ft, Some(4_500));
}

#[test]
fn a_track_without_an_altitude_projects_none() {
    let snapshot = moving_snapshot(1_000_000);
    let projection = project_position(&snapshot, 11_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
    assert_eq!(projection.pressure_altitude_ft, None);
}

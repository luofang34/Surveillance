#![allow(clippy::expect_used, clippy::panic)]

use crate::engine::{EngineConfig, SurveillanceEngine};
use crate::freshness::FreshnessPolicy;
use crate::observation::{
    ObservationOrigin, PositionObservation, VelocityObservation, Wgs84Position,
};
use crate::projection::{ProjectionBasis, ProjectionBounds, project_position};
use crate::source::Band;

use super::{ICAO, PRODUCER, engine, ingest, observation};

#[test]
fn an_engine_snapshot_carries_its_projection_bounds() {
    let mut engine = engine();
    let mut observed = observation(
        1_000_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    observed.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 47.6,
        longitude_deg: -122.3,
    }));
    observed.velocity = Some(VelocityObservation {
        ground_speed_kt: Some(120.0),
        track_angle_deg_true: Some(0.0),
        vertical_rate_fpm: Some(600),
        ..VelocityObservation::default()
    });
    observed.pressure_altitude_ft = Some(4_500);
    ingest(&mut engine, observed);

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert_eq!(
        snapshot.projection_bounds,
        Some(ProjectionBounds::from_freshness(&FreshnessPolicy::default()))
    );

    let projection = project_position(&snapshot, 11_000_000).expect("a positioned track projects");
    assert_eq!(projection.basis, ProjectionBasis::Extrapolated);
    assert_eq!(projection.pressure_altitude_ft, Some(4_600));
}

#[test]
fn a_custom_policy_reaches_the_snapshot_bounds() {
    let custom = EngineConfig {
        freshness: FreshnessPolicy {
            extrapolate_max_micros: 7_000_000,
            ..FreshnessPolicy::default()
        },
        ..EngineConfig::default()
    };
    let mut engine = SurveillanceEngine::new(custom, PRODUCER).expect("test config is valid");
    ingest(
        &mut engine,
        observation(
            1_000_000,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        ),
    );

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert_eq!(
        snapshot.projection_bounds,
        Some(ProjectionBounds::from_freshness(&custom.freshness))
    );
}

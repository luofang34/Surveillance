#![allow(clippy::expect_used, clippy::panic)]

use crate::engine::tests::{ICAO, engine, ingest, observation};
use crate::engine::{FieldDisposition, SurveillanceEngine};
use crate::freshness::FreshnessPolicy;
use crate::observation::{AirGroundState, ObservationOrigin, TrafficObservation};
use crate::source::Band;

const UAT: ObservationOrigin = ObservationOrigin::AdsbDirect { band: Band::Uat978 };
const ES: ObservationOrigin = ObservationOrigin::AdsbDirect {
    band: Band::Adsb1090,
};

fn report(at: u64, origin: ObservationOrigin, state: AirGroundState) -> TrafficObservation {
    let mut report = observation(at, ICAO, origin);
    report.air_ground = Some(state);
    report
}

fn fused(engine: &SurveillanceEngine) -> AirGroundState {
    engine
        .snapshot(ICAO)
        .expect("track exists")
        .air_ground
        .as_ref()
        .expect("the track has an air/ground state")
        .value
}

#[test]
fn a_generic_airborne_report_does_not_displace_a_fresh_scaling_regime() {
    let mut engine = engine();
    ingest(&mut engine, report(1_000, UAT, AirGroundState::Subsonic));

    for step in 1..=4_u64 {
        let generic = report(1_000 + step * 500_000, ES, AirGroundState::Airborne);
        let mut deltas = 0_u32;
        let outcome = engine
            .ingest(generic, |_| deltas += 1)
            .expect("ingest succeeds");
        assert_eq!(outcome.fields.air_ground, FieldDisposition::Rejected);
        assert_eq!(deltas, 0, "an implied state is not a change");
        assert_eq!(fused(&engine), AirGroundState::Subsonic);

        let specific = report(1_250_000 + step * 500_000, UAT, AirGroundState::Subsonic);
        assert!(ingest(&mut engine, specific).is_empty());
    }
}

#[test]
fn a_scaling_regime_replaces_a_generic_airborne_report() {
    let mut engine = engine();
    ingest(&mut engine, report(1_000, ES, AirGroundState::Airborne));

    let deltas = ingest(&mut engine, report(2_000, UAT, AirGroundState::Supersonic));

    assert_eq!(deltas.len(), 1);
    assert_eq!(fused(&engine), AirGroundState::Supersonic);
}

#[test]
fn a_generic_airborne_report_replaces_a_stale_scaling_regime() {
    let window = FreshnessPolicy::default().identity_fresh_micros;
    let mut engine = engine();
    ingest(&mut engine, report(1_000, UAT, AirGroundState::Subsonic));

    let deltas = ingest(
        &mut engine,
        report(1_001 + window, ES, AirGroundState::Airborne),
    );

    assert_eq!(deltas.len(), 1);
    assert_eq!(fused(&engine), AirGroundState::Airborne);
}

#[test]
fn a_stronger_origin_keeps_precedence_over_a_scaling_regime() {
    let tisb = ObservationOrigin::Tisb { band: Band::Uat978 };
    let mut engine = engine();
    ingest(&mut engine, report(1_000, tisb, AirGroundState::Subsonic));

    ingest(&mut engine, report(2_000, ES, AirGroundState::Airborne));

    assert_eq!(fused(&engine), AirGroundState::Airborne);
}

#[test]
fn a_ground_report_replaces_an_airborne_scaling_regime() {
    let mut engine = engine();
    ingest(&mut engine, report(1_000, UAT, AirGroundState::Subsonic));

    ingest(&mut engine, report(2_000, ES, AirGroundState::Ground));

    assert_eq!(fused(&engine), AirGroundState::Ground);
}

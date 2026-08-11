#![allow(clippy::expect_used, clippy::panic)]

use crate::cpr::{CprDecodeError, CprFormat, CprFrame};
use crate::engine::tests::{ICAO, PRODUCER, advance, engine, ingest, observation};
use crate::engine::{
    EngineConfig, FieldDisposition, PositionDisposition, PositionRejection, SurveillanceEngine,
};
use crate::observation::{ObservationOrigin, ObservationTime, PositionObservation, Wgs84Position};
use crate::source::Band;
use crate::track::{PositionStatus, TrackDelta};

const EVEN: CprFrame = CprFrame {
    format: CprFormat::Even,
    encoded_latitude: 93_000,
    encoded_longitude: 51_372,
};
const ODD: CprFrame = CprFrame {
    format: CprFormat::Odd,
    encoded_latitude: 74_158,
    encoded_longitude: 50_194,
};

const TRUTH: Wgs84Position = Wgs84Position {
    latitude_deg: 52.257_202,
    longitude_deg: 3.919_372,
};

fn cpr_observation(at: u64, frame: CprFrame) -> crate::observation::TrafficObservation {
    let mut observed = observation(
        at,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    observed.position = Some(PositionObservation::CprAirborne(frame));
    observed
}

fn assert_near(actual: Wgs84Position, expected: Wgs84Position) {
    assert!(
        (actual.latitude_deg - expected.latitude_deg).abs() < 1e-4
            && (actual.longitude_deg - expected.longitude_deg).abs() < 1e-4,
        "expected {expected:?}, applied {actual:?}"
    );
}

#[test]
fn a_single_cpr_frame_leaves_the_track_position_absent() {
    let mut engine = engine();
    let deltas = ingest(&mut engine, cpr_observation(1_000, ODD));

    let TrackDelta::Created(snapshot) = &deltas[0] else {
        panic!("expected a creation, got {:?}", deltas[0]);
    };
    assert!(
        snapshot.position.is_none(),
        "one frame cannot decode a global position"
    );
    assert_eq!(snapshot.position_status, PositionStatus::Unavailable);
}

#[test]
fn a_complete_pair_gives_the_track_its_first_position() {
    let mut engine = engine();
    ingest(&mut engine, cpr_observation(1_000, ODD));
    let deltas = ingest(&mut engine, cpr_observation(2_000, EVEN));

    assert_eq!(deltas.len(), 1);
    let TrackDelta::Updated(snapshot) = &deltas[0] else {
        panic!("the first position must update, got {:?}", deltas[0]);
    };
    let position = snapshot.position.as_ref().expect("position applied");
    assert_eq!(snapshot.position_status, PositionStatus::Current);
    assert_near(position.value, TRUTH);
    assert_eq!(position.observed_at_micros(), 2_000);
    assert_eq!(
        position.provenance.origin,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090
        }
    );
}

#[test]
fn cpr_pair_age_uses_reception_time() {
    let received_at_micros = 100_000_000;
    let mut odd = cpr_observation(received_at_micros, ODD);
    odd.time = ObservationTime::mapped(received_at_micros, 0, 0);
    let mut even = cpr_observation(received_at_micros + 1_000, EVEN);
    even.time = ObservationTime::mapped(
        received_at_micros + 1_000,
        engine().config().cpr.max_pair_age_micros + 1,
        0,
    );

    let mut engine = engine();
    ingest(&mut engine, odd);
    let outcome = engine
        .ingest(even, |_| {})
        .expect("the close receptions form a valid pair");

    assert!(matches!(outcome.position, PositionDisposition::Applied(_)));
    assert!(
        engine
            .snapshot(ICAO)
            .expect("track exists")
            .position
            .is_some()
    );
}

#[test]
fn a_stale_pair_does_not_produce_a_position() {
    let mut engine = engine();
    let too_late = 1_000 + engine.config().cpr.max_pair_age_micros + 1;
    ingest(&mut engine, cpr_observation(1_000, ODD));
    ingest(&mut engine, cpr_observation(too_late, EVEN));

    assert!(
        engine
            .snapshot(ICAO)
            .expect("track exists")
            .position
            .is_none(),
        "a pair spanning more than the pairing window must not decode"
    );
}

#[test]
fn ingest_reports_pending_and_stale_cpr_separately() {
    let mut engine = engine();
    let pending = engine
        .ingest(cpr_observation(1_000, ODD), |_| {})
        .expect("the valid frame reaches CPR history");
    assert_eq!(
        pending.position,
        PositionDisposition::Pending(CprDecodeError::PairIncomplete)
    );

    let too_late = 1_000 + engine.config().cpr.max_pair_age_micros + 1;
    let mut observation = cpr_observation(too_late, EVEN);
    observation.pressure_altitude_ft = Some(12_000);
    let stale = engine
        .ingest(observation, |_| {})
        .expect("altitude still applies when CPR is stale");
    assert!(matches!(
        stale.position,
        PositionDisposition::Rejected(PositionRejection::Decode(CprDecodeError::PairTooOld { .. }))
    ));
    assert_eq!(stale.fields.pressure_altitude_ft, FieldDisposition::Changed);
    assert_eq!(stale.rejected_field_count(), 1);
}

#[test]
fn a_decoded_position_applies_without_any_pairing() {
    let mut engine = engine();
    let mut observed = observation(
        1_000,
        ICAO,
        ObservationOrigin::AdsbDirect { band: Band::Uat978 },
    );
    observed.position = Some(PositionObservation::Decoded(TRUTH));
    ingest(&mut engine, observed);

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    let position = snapshot.position.as_ref().expect("position applied");
    assert_near(position.value, TRUTH);
    assert_eq!(
        position.provenance.origin,
        ObservationOrigin::AdsbDirect { band: Band::Uat978 }
    );
}

#[test]
fn an_implausible_decoded_position_is_rejected() {
    let mut engine = engine();
    let mut first = observation(
        1_000,
        ICAO,
        ObservationOrigin::AdsbDirect { band: Band::Uat978 },
    );
    first.position = Some(PositionObservation::Decoded(TRUTH));
    ingest(&mut engine, first);

    let mut jumped = observation(
        1_100_000,
        ICAO,
        ObservationOrigin::AdsbDirect { band: Band::Uat978 },
    );
    jumped.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: -33.9,
        longitude_deg: 151.2,
    }));
    assert!(
        ingest(&mut engine, jumped).is_empty(),
        "a jump across the globe in one second must not apply"
    );

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    let position = snapshot.position.as_ref().expect("position retained");
    assert_near(position.value, TRUTH);
}

#[test]
fn position_travel_time_uses_reception_time() {
    let mut engine = engine();
    let mut first = observation(
        0,
        ICAO,
        ObservationOrigin::AdsbDirect { band: Band::Uat978 },
    );
    first.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 0.0,
        longitude_deg: 0.0,
    }));
    ingest(&mut engine, first);

    let mut later = observation(
        300_000_000,
        ICAO,
        ObservationOrigin::AdsbDirect { band: Band::Uat978 },
    );
    later.time = ObservationTime::mapped(300_000_000, 1, 0);
    later.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 1.0,
        longitude_deg: 0.0,
    }));
    let outcome = engine
        .ingest(later, |_| {})
        .expect("the observation is valid");

    assert!(matches!(outcome.position, PositionDisposition::Applied(_)));
}

#[test]
fn an_established_track_decodes_a_lone_frame_against_its_own_position() {
    let mut engine = engine();
    let mut seeded = observation(
        1_000,
        ICAO,
        ObservationOrigin::AdsbDirect { band: Band::Uat978 },
    );
    seeded.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 52.2560,
        longitude_deg: 3.9160,
    }));
    ingest(&mut engine, seeded);

    ingest(&mut engine, cpr_observation(1_000_000, EVEN));

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    let position = snapshot.position.as_ref().expect("position applied");
    assert_near(position.value, TRUTH);
}

#[test]
fn a_lone_frame_resolved_far_from_its_reference_is_rejected() {
    let mut engine = engine();
    let mut seeded = observation(
        1_000,
        ICAO,
        ObservationOrigin::AdsbDirect { band: Band::Uat978 },
    );
    let stale_reference = Wgs84Position {
        latitude_deg: 52.0,
        longitude_deg: 3.8,
    };
    seeded.position = Some(PositionObservation::Decoded(stale_reference));
    ingest(&mut engine, seeded);

    assert!(
        ingest(&mut engine, cpr_observation(1_500, EVEN)).is_empty(),
        "a local decode 29 km from the position half a millisecond earlier is not reachable"
    );
    assert_near(
        engine
            .snapshot(ICAO)
            .expect("track exists")
            .position
            .as_ref()
            .expect("position retained")
            .value,
        stale_reference,
    );
}

#[test]
fn a_surface_frame_needs_a_reference_position() {
    let mut engine = engine();
    let mut observed = observation(
        1_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    observed.position = Some(PositionObservation::CprSurface(EVEN));
    ingest(&mut engine, observed.clone());

    assert!(
        engine
            .snapshot(ICAO)
            .expect("track exists")
            .position
            .is_none(),
        "a surface frame must not decode without a reference"
    );

    // The same frame resolves into whichever quadrant the reference names,
    // which is why surface frames cannot decode without one.
    for reference in [
        Wgs84Position {
            latitude_deg: 52.3,
            longitude_deg: 4.7,
        },
        Wgs84Position {
            latitude_deg: -33.9,
            longitude_deg: 151.2,
        },
    ] {
        let mut referenced = SurveillanceEngine::new(EngineConfig::default(), PRODUCER)
            .expect("default config is valid");
        referenced
            .set_reference_position(Some(reference))
            .expect("the reference is valid");
        ingest(&mut referenced, observed.clone());

        let decoded = referenced
            .snapshot(ICAO)
            .expect("track exists")
            .position
            .as_ref()
            .expect("position applied")
            .value;
        assert!(
            (decoded.latitude_deg - reference.latitude_deg).abs() <= 0.75
                && (decoded.longitude_deg - reference.longitude_deg).abs() <= 1.25,
            "a surface decode must land within half a zone of {reference:?}, got {decoded:?}"
        );
    }
}

#[test]
fn a_position_absent_track_gains_a_position_and_then_expires() {
    let mut engine = engine();
    let mut identified = observation(0, ICAO, ObservationOrigin::ModeSReply);
    identified.pressure_altitude_ft = Some(1_200);
    let created = ingest(&mut engine, identified);
    let id = created[0].id();
    assert!(
        engine
            .snapshot(ICAO)
            .expect("track exists")
            .position
            .is_none()
    );

    ingest(&mut engine, cpr_observation(1_000, ODD));
    ingest(&mut engine, cpr_observation(2_000, EVEN));
    assert!(
        engine
            .snapshot(ICAO)
            .expect("track exists")
            .position
            .is_some()
    );

    let retention = engine.config().freshness.remove_after_micros;
    let removed = advance(&mut engine, retention + 1);
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].id(), id);
}

use crate::cpr::{CprFormat, CprFrame};
use crate::engine::FieldDisposition;
use crate::observation::{
    AdsbIntegrityDeclaration, AdsbPositionIntegrity, AdsbVersion, ObservationOrigin,
    PositionObservation,
};
use crate::source::Band;
use crate::track::TrackDelta;

use super::{ICAO, engine, ingest, observation};

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

fn direct(at_micros: u64) -> crate::TrafficObservation {
    observation(
        at_micros,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    )
}

fn declaration(at_micros: u64, supplement_a: bool) -> crate::TrafficObservation {
    let mut observed = direct(at_micros);
    observed.adsb_integrity = Some(
        AdsbIntegrityDeclaration::new(
            AdsbVersion::Do260B,
            Some(supplement_a),
            None,
            Some(9),
            Some(3),
        )
        .expect("declaration is valid"),
    );
    observed
}

fn position(at_micros: u64, frame: CprFrame, supplement_b: bool) -> crate::TrafficObservation {
    let mut observed = direct(at_micros);
    observed.position = Some(PositionObservation::CprAirborne(frame));
    observed.adsb_position_integrity = Some(
        AdsbPositionIntegrity::airborne(11, Some(supplement_b)).expect("type code is airborne"),
    );
    observed
}

#[test]
fn retained_status_qualifies_later_position_messages() {
    let mut engine = engine();
    ingest(&mut engine, declaration(500, false));
    ingest(&mut engine, position(1_000, ODD, false));
    let deltas = ingest(&mut engine, position(2_000, EVEN, false));

    assert!(matches!(deltas.as_slice(), [TrackDelta::Updated(_)]));
    let snapshot = engine.snapshot(ICAO).expect("track exists");
    let quality = snapshot.position.as_ref().expect("position exists").quality;
    assert_eq!(quality.nic, Some(8));
    assert_eq!(quality.nac_p, Some(9));
    assert_eq!(quality.sil, Some(3));
}

#[test]
fn a_position_without_observed_version_has_no_derived_quality() {
    let mut engine = engine();
    ingest(&mut engine, position(1_000, ODD, false));
    ingest(&mut engine, position(2_000, EVEN, false));

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert_eq!(
        snapshot.position.as_ref().expect("position exists").quality,
        crate::FieldQuality::default()
    );
}

#[test]
fn later_status_updates_the_current_positions_quality() {
    let mut engine = engine();
    ingest(&mut engine, position(1_000, ODD, false));
    ingest(&mut engine, position(2_000, EVEN, false));

    let mut deltas = Vec::new();
    let outcome = engine
        .ingest(declaration(2_500, false), |delta| {
            deltas.push(delta.clone())
        })
        .expect("status applies");

    assert_eq!(outcome.fields.adsb_integrity, FieldDisposition::Changed);
    assert!(matches!(deltas.as_slice(), [TrackDelta::Updated(_)]));
    let snapshot = engine.snapshot(ICAO).expect("track exists");
    let quality = snapshot.position.as_ref().expect("position exists").quality;
    assert_eq!(quality.nic, Some(8));
    assert_eq!(quality.nac_p, Some(9));
    assert_eq!(quality.sil, Some(3));
}

#[test]
fn inconsistent_supplements_leave_only_independent_indicators() {
    let mut engine = engine();
    ingest(&mut engine, declaration(500, false));
    ingest(&mut engine, position(1_000, ODD, true));
    ingest(&mut engine, position(2_000, EVEN, true));

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    let quality = snapshot.position.as_ref().expect("position exists").quality;
    assert_eq!(quality.nic, None);
    assert_eq!(quality.nac_p, Some(9));
    assert_eq!(quality.sil, Some(3));
}

#[test]
fn an_expired_declaration_does_not_qualify_a_new_position() {
    let mut engine = engine();
    ingest(&mut engine, declaration(1, false));
    let after_expiry = engine.config().freshness.integrity_fresh_micros + 2;
    ingest(&mut engine, position(after_expiry, ODD, false));
    ingest(&mut engine, position(after_expiry + 1_000, EVEN, false));

    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert_eq!(
        snapshot.position.as_ref().expect("position exists").quality,
        crate::FieldQuality::default()
    );
}
use alloc::vec::Vec;

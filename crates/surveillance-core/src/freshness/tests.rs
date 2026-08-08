#![allow(clippy::expect_used, clippy::panic)]

use crate::observation::ObservationTime;
use crate::source::Band;
use crate::source::SourceRef;

use crate::freshness::{FieldProvenance, FieldQuality, TimedField};
use crate::observation::ObservationOrigin;
use crate::track::AddressNamespace;

fn provenance() -> FieldProvenance {
    FieldProvenance::new(
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
        AddressNamespace::Icao,
        SourceRef::default(),
    )
}

#[test]
fn age_saturates_before_the_observation_time() {
    let field = TimedField::new(
        1_i32,
        ObservationTime::local(10_000),
        FieldQuality::default(),
        provenance(),
    );
    assert_eq!(field.age_micros(9_000), 0);
    assert_eq!(field.age_micros(10_000), 0);
    assert_eq!(field.age_micros(25_000), 15_000);
}

#[test]
fn freshness_is_inclusive_of_the_window_boundary() {
    let field = TimedField::new(
        1_i32,
        ObservationTime::local(10_000),
        FieldQuality::default(),
        provenance(),
    );
    assert!(field.is_fresh(15_000, 5_000));
    assert!(!field.is_fresh(15_001, 5_000));
}

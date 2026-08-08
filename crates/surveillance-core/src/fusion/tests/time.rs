#![allow(clippy::expect_used, clippy::panic)]

//! How establishment, uncertainty, and skew of an observation's time affect
//! which source wins a field.

use crate::freshness::{FieldProvenance, FieldQuality, TimedField};
use crate::fusion::{FieldUpdate, fuse};
use crate::observation::{ObservationOrigin, ObservationTime};
use crate::source::{Band, DeliveryPath, SourceRef};
use crate::track::AddressNamespace;

const WINDOW: u64 = 10_000;

/// A provider report that arrives after a local one, but describes an older
/// moment, must not overwrite it.
#[test]
fn a_late_network_report_cannot_overwrite_a_newer_local_observation() {
    let local = TimedField::new(
        2,
        ObservationTime::local(5_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            AddressNamespace::Icao,
            SourceRef::new(1, 0, DeliveryPath::LocalReceiver),
        ),
    );
    // Arrives at 6000 but describes 4000: older than what is already applied.
    let late = TimedField::new(
        7,
        ObservationTime::mapped(6_000, 4_000, 0),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::ProviderFused,
            AddressNamespace::Icao,
            SourceRef::new(2, 0, DeliveryPath::NetworkProvider),
        ),
    );

    let mut slot = Some(local);
    assert_eq!(fuse(&mut slot, late, WINDOW), FieldUpdate::Rejected);
    assert_eq!(slot.expect("value retained").value, 2);
}

/// Arrival order alone grants nothing: the same provider report describing a
/// *newer* moment than the applied value still loses on origin while the
/// local value is fresh, because origin decides between fresh values.
#[test]
fn delivery_path_never_grants_authority_on_its_own() {
    let local = TimedField::new(
        2,
        ObservationTime::local(5_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            AddressNamespace::Icao,
            SourceRef::new(1, 0, DeliveryPath::LocalReceiver),
        ),
    );
    let provider = TimedField::new(
        7,
        ObservationTime::mapped(6_000, 5_500, 0),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::ProviderFused,
            AddressNamespace::Icao,
            SourceRef::new(2, 0, DeliveryPath::NetworkProvider),
        ),
    );

    let mut slot = Some(local);
    assert_eq!(fuse(&mut slot, provider, WINDOW), FieldUpdate::Rejected);
}

/// A provider whose mapped time is trustworthy does participate once the
/// local value has aged out of its window.
#[test]
fn a_trustworthy_provider_report_wins_a_field_the_local_source_let_go_stale() {
    let local = TimedField::new(
        2,
        ObservationTime::local(1_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            AddressNamespace::Icao,
            SourceRef::new(1, 0, DeliveryPath::LocalReceiver),
        ),
    );
    let stale_by_one = 1_000 + WINDOW + 1;
    let provider = TimedField::new(
        7,
        ObservationTime::mapped(stale_by_one, stale_by_one, 0),
        FieldQuality::new(Some(9), None, None, None),
        FieldProvenance::new(
            ObservationOrigin::ProviderFused,
            AddressNamespace::Icao,
            SourceRef::new(2, 0, DeliveryPath::NetworkProvider),
        ),
    );

    let mut slot = Some(local);
    assert_eq!(fuse(&mut slot, provider, WINDOW), FieldUpdate::Changed);
    assert_eq!(slot.expect("value replaced").value, 7);
}

/// A source that will not say when it measured has said nothing about age,
/// and must never be read as though it had measured on arrival.
#[test]
fn an_unestablished_time_cannot_displace_an_established_fresh_value() {
    let established = TimedField::new(
        2,
        ObservationTime::local(5_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            AddressNamespace::Icao,
            SourceRef::new(1, 0, DeliveryPath::LocalReceiver),
        ),
    );
    // Higher-ranked origin, later arrival — and still refused, because its
    // timing is unknown while the applied value is established and fresh.
    let timeless = TimedField::new(
        7,
        ObservationTime::unknown_source_time(6_000),
        FieldQuality::new(Some(9), Some(9), Some(9), Some(9)),
        FieldProvenance::new(
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            AddressNamespace::Icao,
            SourceRef::new(2, 0, DeliveryPath::NetworkProvider),
        ),
    );

    let mut slot = Some(established);
    assert_eq!(fuse(&mut slot, timeless, WINDOW), FieldUpdate::Rejected);
    assert_eq!(slot.expect("value retained").value, 2);
}

/// It is not useless, though: once nothing established is current, it is the
/// only evidence there is.
#[test]
fn an_unestablished_time_still_fills_a_field_nothing_else_holds() {
    let mut empty = None;
    let timeless = TimedField::new(
        7,
        ObservationTime::unknown_source_time(6_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::Unknown,
            AddressNamespace::Icao,
            SourceRef::new(2, 0, DeliveryPath::NetworkProvider),
        ),
    );
    assert_eq!(fuse(&mut empty, timeless, WINDOW), FieldUpdate::Changed);
}

/// Timing uncertainty ages a value: a loosely mapped time is treated as
/// older than a tightly mapped one, never as equally current.
#[test]
fn mapping_uncertainty_counts_against_freshness() {
    let precise = ObservationTime::mapped(10_000, 10_000, 0);
    let loose = ObservationTime::mapped(10_000, 10_000, 4_000);

    assert_eq!(precise.age_micros(11_000), 1_000);
    assert_eq!(loose.age_micros(11_000), 5_000);
    assert!(precise.is_fresh(11_000, 2_000));
    assert!(
        !loose.is_fresh(11_000, 2_000),
        "an uncertain mapping must not claim the freshness of a precise one"
    );
}

/// A source clock running ahead cannot claim to have measured the future and
/// outrank live local data.
#[test]
fn a_source_clock_ahead_of_ingress_is_clamped() {
    let skewed = ObservationTime::mapped(5_000, 9_999, 0);
    assert_eq!(skewed.observed_at_micros, 5_000);
    assert_eq!(skewed.received_at_micros, 5_000);
}

/// An arrival stamp is not a measurement time and must never act as an age
/// floor. A source reporting faster than a trustworthy source's mapping
/// latency would otherwise hold the field against it indefinitely.
#[test]
fn an_unestablished_time_never_blocks_an_established_one() {
    let timeless = TimedField::new(
        2,
        ObservationTime::unknown_source_time(1_000_000),
        FieldQuality::default(),
        FieldProvenance::new(
            ObservationOrigin::Unknown,
            AddressNamespace::Icao,
            SourceRef::new(2, 0, DeliveryPath::NetworkProvider),
        ),
    );
    let trustworthy = TimedField::new(
        7,
        ObservationTime::mapped(1_001_000, 999_000, 0),
        FieldQuality::new(Some(9), Some(9), Some(9), Some(9)),
        FieldProvenance::new(
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            AddressNamespace::Icao,
            SourceRef::new(1, 0, DeliveryPath::LocalReceiver),
        ),
    );

    let mut slot = Some(timeless);
    assert_eq!(fuse(&mut slot, trustworthy, WINDOW), FieldUpdate::Changed);
    assert_eq!(slot.expect("value replaced").value, 7);
}

/// A fast timeless source must not be able to lock a trustworthy one out by
/// cadence alone.
#[test]
fn a_fast_timeless_source_cannot_lock_out_a_trustworthy_one() {
    let mut slot = None;
    let mut accepted = 0;

    for round in 0..50_u64 {
        let now = 1_000_000 + round * 1_000;
        fuse(
            &mut slot,
            TimedField::new(
                2,
                ObservationTime::unknown_source_time(now),
                FieldQuality::default(),
                FieldProvenance::new(
                    ObservationOrigin::Unknown,
                    AddressNamespace::Icao,
                    SourceRef::new(2, 0, DeliveryPath::NetworkProvider),
                ),
            ),
            WINDOW,
        );
        // The trustworthy source measured 5 ms before it could report.
        let outcome = fuse(
            &mut slot,
            TimedField::new(
                7,
                ObservationTime::mapped(now, now.saturating_sub(5_000), 0),
                FieldQuality::default(),
                FieldProvenance::new(
                    ObservationOrigin::AdsbDirect {
                        band: Band::Adsb1090,
                    },
                    AddressNamespace::Icao,
                    SourceRef::new(1, 0, DeliveryPath::LocalReceiver),
                ),
            ),
            WINDOW,
        );
        if outcome != FieldUpdate::Rejected {
            accepted += 1;
        }
    }

    assert_eq!(
        accepted, 50,
        "every genuine report must reach the field, whatever the other source's cadence"
    );
}

/// Which of two observations survives cannot depend on which happened to be
/// applied first.
#[test]
fn the_winner_does_not_depend_on_arrival_order() {
    fn established() -> TimedField<i32> {
        TimedField::new(
            2,
            ObservationTime::local(5_000),
            FieldQuality::default(),
            FieldProvenance::new(
                ObservationOrigin::AdsbDirect {
                    band: Band::Adsb1090,
                },
                AddressNamespace::Icao,
                SourceRef::new(1, 0, DeliveryPath::LocalReceiver),
            ),
        )
    }
    fn timeless() -> TimedField<i32> {
        TimedField::new(
            7,
            ObservationTime::unknown_source_time(5_000),
            FieldQuality::default(),
            FieldProvenance::new(
                ObservationOrigin::ProviderFused,
                AddressNamespace::Icao,
                SourceRef::new(2, 0, DeliveryPath::NetworkProvider),
            ),
        )
    }

    let mut established_first = Some(established());
    fuse(&mut established_first, timeless(), WINDOW);

    let mut timeless_first = Some(timeless());
    fuse(&mut timeless_first, established(), WINDOW);

    assert_eq!(
        established_first.expect("value present").value,
        timeless_first.expect("value present").value,
        "the established observation wins regardless of which arrived first"
    );
    assert_eq!(established_first.expect("value present").value, 2);
}

/// The clamp is a property of the type, not of one constructor: a recorded
/// value cannot carry a measurement time from after its own arrival.
#[test]
fn a_decoded_time_from_the_future_is_clamped_to_its_arrival() {
    let forged = r#"{"received_at_micros":5000,"observed_at_micros":9999999,"basis":{"kind":"mapped","uncertainty_micros":0}}"#;
    let decoded: ObservationTime = serde_json::from_str(forged).expect("wire form decodes");

    assert_eq!(
        decoded.observed_at_micros, 5_000,
        "a source clock running ahead cannot claim to have measured the future"
    );
    // Before the clamp it stayed maximally fresh until wall time caught up
    // with its forged stamp; now it ages from its arrival like anything else.
    assert_eq!(decoded.age_micros(9_000), 4_000);
    assert!(!decoded.is_fresh(9_000, 1_000));
}

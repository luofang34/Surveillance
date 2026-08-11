use crate::engine::IngestError;
use crate::freshness::FreshnessPolicy;
use crate::observation::{EmitterCategory, ObservationOrigin, PositionObservation, Wgs84Position};
use crate::source::Band;
use crate::track::{AddressNamespace, TrackDelta, TrackKey};

use super::{advance, engine, ingest, observation};

const ADDRESS: u32 = 0x00AB_CDEF;
const UNCLASSIFIED: TrackKey = TrackKey::new(AddressNamespace::AdsbNonIcao, ADDRESS);

fn positioned(at_micros: u64, key: TrackKey) -> crate::TrafficObservation {
    let mut observed = observation(
        at_micros,
        key,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    observed.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 40.0,
        longitude_deg: -73.0,
    }));
    observed.pressure_altitude_ft = Some(5_000);
    observed
}

#[test]
fn position_alone_keeps_a_non_icao_address_unclassified() {
    let mut engine = engine();
    ingest(&mut engine, positioned(1_000, UNCLASSIFIED));

    let snapshot = engine.snapshot(UNCLASSIFIED).expect("track exists");
    assert_eq!(snapshot.key, UNCLASSIFIED);
    assert_eq!(snapshot.emitter_category, None);
    assert_eq!(engine.track_count(), 1);
}

#[test]
fn a_surface_category_rekeys_without_losing_state() {
    let mut engine = engine();
    let created = ingest(&mut engine, positioned(1_000, UNCLASSIFIED));
    let original_id = created[0].id();

    let mut classified = observation(
        2_000,
        UNCLASSIFIED,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    classified.emitter_category = Some(EmitterCategory::SurfaceEmergencyVehicle);
    let deltas = ingest(&mut engine, classified);

    let surface = TrackKey::new(AddressNamespace::SurfaceVehicle, ADDRESS);
    assert!(matches!(deltas.as_slice(), [TrackDelta::Updated(_)]));
    let snapshot = engine.snapshot(surface).expect("classified key resolves");
    assert_eq!(snapshot.id, original_id);
    assert_eq!(snapshot.key, surface);
    assert!(snapshot.position.is_some(), "position survives re-keying");
    assert!(snapshot.identities.contains(UNCLASSIFIED));
    assert_eq!(
        engine.snapshot(UNCLASSIFIED).map(|value| value.id),
        Some(original_id),
        "the wire key remains an alias"
    );
    assert!(!crate::may_correlate(
        surface.namespace,
        AddressNamespace::Icao
    ));
}

#[test]
fn an_aircraft_category_enables_aircraft_correlation_policy() {
    let mut engine = engine();
    ingest(&mut engine, positioned(1_000, UNCLASSIFIED));

    let mut classified = observation(
        2_000,
        UNCLASSIFIED,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    classified.emitter_category = Some(EmitterCategory::LightAircraft);
    let deltas = ingest(&mut engine, classified);

    let aircraft = TrackKey::new(AddressNamespace::SelfAssigned, ADDRESS);
    assert!(matches!(deltas.as_slice(), [TrackDelta::Updated(_)]));
    assert_eq!(
        engine.snapshot(aircraft).map(|value| value.key),
        Some(aircraft)
    );
    assert!(crate::may_correlate(
        aircraft.namespace,
        AddressNamespace::Icao
    ));
}

#[test]
fn an_existing_classified_track_retains_and_releases_its_wire_alias() {
    let mut engine = engine();
    let classified_key = TrackKey::new(AddressNamespace::SelfAssigned, ADDRESS);
    let created = ingest(&mut engine, positioned(1_000, classified_key));
    let original_id = created[0].id();

    let mut wire_report = observation(
        2_000,
        UNCLASSIFIED,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    wire_report.emitter_category = Some(EmitterCategory::LightAircraft);
    ingest(&mut engine, wire_report);

    let snapshot = engine
        .snapshot(classified_key)
        .expect("classified track exists");
    assert_eq!(snapshot.id, original_id);
    assert!(snapshot.identities.contains(UNCLASSIFIED));
    assert_eq!(
        engine.snapshot(UNCLASSIFIED).map(|value| value.id),
        Some(original_id)
    );

    advance(
        &mut engine,
        2_000 + FreshnessPolicy::default().remove_after_micros + 1,
    );
    assert_eq!(engine.snapshot(classified_key), None);
    assert_eq!(engine.snapshot(UNCLASSIFIED), None);

    let recreated = ingest(&mut engine, positioned(70_000_000, UNCLASSIFIED));
    assert!(matches!(recreated.as_slice(), [TrackDelta::Created(_)]));
    assert_ne!(recreated[0].id(), original_id);
}

#[test]
fn an_older_category_cannot_reverse_a_classification() {
    let mut engine = engine();
    let mut aircraft = positioned(3_000, UNCLASSIFIED);
    aircraft.emitter_category = Some(EmitterCategory::LightAircraft);
    ingest(&mut engine, aircraft);

    let mut older = observation(
        2_000,
        UNCLASSIFIED,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    older.emitter_category = Some(EmitterCategory::SurfaceServiceVehicle);
    assert!(ingest(&mut engine, older).is_empty());

    let key = TrackKey::new(AddressNamespace::SelfAssigned, ADDRESS);
    let snapshot = engine.snapshot(key).expect("aircraft key remains primary");
    assert_eq!(snapshot.key, key);
    assert_eq!(
        snapshot.emitter_category.as_ref().map(|field| field.value),
        Some(EmitterCategory::LightAircraft)
    );
}

#[test]
fn classification_refuses_two_existing_tracks_with_the_same_address() {
    let mut engine = engine();
    ingest(&mut engine, positioned(1_000, UNCLASSIFIED));
    let surface = TrackKey::new(AddressNamespace::SurfaceVehicle, ADDRESS);
    ingest(&mut engine, positioned(1_100, surface));

    let mut classified = observation(
        2_000,
        UNCLASSIFIED,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
    );
    classified.emitter_category = Some(EmitterCategory::SurfaceServiceVehicle);
    let result = engine.ingest(classified, |_| {});

    assert!(matches!(
        result,
        Err(IngestError::IdentityClassificationConflict { .. })
    ));
    assert_eq!(engine.track_count(), 2);
    assert_eq!(
        engine
            .snapshot(UNCLASSIFIED)
            .and_then(|snapshot| snapshot.emitter_category.as_ref().cloned()),
        None,
        "the rejected observation cannot mutate either track"
    );
}

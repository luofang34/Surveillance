#![allow(clippy::expect_used, clippy::panic)]

use alloc::vec::Vec;

use crate::source::Band;

use crate::engine::tests::{ICAO, PRODUCER, engine, ingest, observation};
use crate::engine::{EngineConfig, SurveillanceEngine};
use crate::observation::{
    Callsign, ObservationOrigin, PositionObservation, TrafficObservation, VelocityObservation,
    Wgs84Position,
};
use crate::track::{AddressNamespace, RemovalReason, TrackDelta, TrackId, TrackKey, TrackPhase};

const TRACK_FILE: TrackKey = TrackKey::new(AddressNamespace::TisbTrackFile, 0x0000_0042);
const VEHICLE: TrackKey = TrackKey::new(AddressNamespace::SurfaceVehicle, 0x0000_0077);

/// One positioned report, with the altitude and speed correlation needs.
fn report(at: u64, key: TrackKey, link: ObservationOrigin, drift: f64) -> TrafficObservation {
    let mut observed = observation(at, key, link);
    observed.position = Some(PositionObservation::Decoded(Wgs84Position {
        latitude_deg: 40.0 + drift,
        longitude_deg: -74.0,
    }));
    observed.pressure_altitude_ft = Some(10_000);
    observed.velocity = Some(VelocityObservation {
        ground_speed_kt: Some(250.0),
        ..VelocityObservation::default()
    });
    observed
}

/// Feed both identities a run of agreeing reports drifting together.
fn fly_together(
    engine: &mut SurveillanceEngine,
    first: TrackKey,
    second: TrackKey,
    rounds: u32,
) -> Vec<TrackDelta> {
    let mut deltas = Vec::new();
    for round in 0..rounds {
        let at = 1_000_000 + u64::from(round) * 1_000_000;
        let drift = f64::from(round) * 0.0001;
        for (offset, key, link) in [
            (0, first, ObservationOrigin::Tisb { band: Band::Uat978 }),
            (
                1_000,
                second,
                ObservationOrigin::AdsbDirect {
                    band: Band::Adsb1090,
                },
            ),
        ] {
            engine
                .ingest(report(at + offset, key, link, drift), |delta| {
                    deltas.push(delta.clone());
                })
                .expect("ingest succeeds");
        }
    }
    deltas
}

#[test]
fn one_icao_identity_heard_through_two_links_is_one_track() {
    let mut engine = engine();

    let mut rebroadcast = report(
        1_000,
        ICAO,
        ObservationOrigin::AdsR { band: Band::Uat978 },
        0.0,
    );
    rebroadcast.callsign = Some(Callsign::new("DAL123"));
    ingest(&mut engine, rebroadcast);

    let direct = report(
        2_000,
        ICAO,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090,
        },
        0.0001,
    );
    ingest(&mut engine, direct);

    assert_eq!(
        engine.track_count(),
        1,
        "address equality in the ICAO namespace needs no correlation"
    );
    let snapshot = engine.snapshot(ICAO).expect("track exists");
    assert_eq!(
        snapshot
            .position
            .as_ref()
            .expect("position applied")
            .provenance
            .origin,
        ObservationOrigin::AdsbDirect {
            band: Band::Adsb1090
        },
        "the direct link wins the position"
    );
    assert_eq!(
        snapshot
            .callsign
            .as_ref()
            .expect("callsign applied")
            .provenance
            .origin,
        ObservationOrigin::AdsR { band: Band::Uat978 },
        "the rebroadcast keeps the field only it supplied"
    );
}

#[test]
fn a_stale_direct_position_yields_to_a_fresh_rebroadcast() {
    let mut engine = engine();
    ingest(
        &mut engine,
        report(
            1_000,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            0.0,
        ),
    );

    let window = engine.config().freshness.position_fresh_micros;
    ingest(
        &mut engine,
        report(
            1_000 + window + 1,
            ICAO,
            ObservationOrigin::Tisb { band: Band::Uat978 },
            0.001,
        ),
    );

    assert_eq!(
        engine
            .snapshot(ICAO)
            .expect("track exists")
            .position
            .as_ref()
            .expect("position applied")
            .provenance
            .origin,
        ObservationOrigin::Tisb { band: Band::Uat978 }
    );
}

#[test]
fn identical_callsigns_on_different_addresses_never_merge() {
    let mut engine = engine();
    let other = TrackKey::new(AddressNamespace::Icao, 0x0040_621D);

    for key in [ICAO, other] {
        let mut observed = observation(
            1_000,
            key,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
        );
        observed.callsign = Some(Callsign::new("SAME1"));
        ingest(&mut engine, observed);
    }

    assert_eq!(engine.track_count(), 2);
}

#[test]
fn a_track_file_and_an_icao_identity_correlate_into_one_track() {
    let mut engine = engine();
    let created = ingest(
        &mut engine,
        report(
            0,
            TRACK_FILE,
            ObservationOrigin::Tisb { band: Band::Uat978 },
            0.0,
        ),
    );
    let older = created[0].id();

    let deltas = fly_together(&mut engine, TRACK_FILE, ICAO, 6);

    assert_eq!(engine.track_count(), 1, "the pair merged into one track");
    let merged = deltas
        .iter()
        .find_map(|delta| match delta {
            TrackDelta::Removed {
                id,
                reason: RemovalReason::Merged { into },
                ..
            } => Some((*id, *into)),
            _ => None,
        })
        .expect("a merge removal is emitted");
    assert_eq!(merged.1, older, "the track watched longest survives");
    assert_ne!(merged.0, older);

    let snapshot = engine.snapshot_by_id(older).expect("survivor exists");
    assert_eq!(
        snapshot.key, ICAO,
        "a merged track presents its cooperative identity"
    );
    assert!(snapshot.identities.contains(TRACK_FILE));
    assert!(snapshot.identities.contains(ICAO));
    assert_eq!(
        snapshot.created_at_micros, 0,
        "the merged track keeps the earlier history"
    );
    assert_eq!(
        engine.snapshot(TRACK_FILE).map(|snapshot| snapshot.id),
        Some(older),
        "the absorbed identity still resolves to the survivor"
    );
}

#[test]
fn one_coincidental_alignment_does_not_merge() {
    let mut engine = engine();
    ingest(
        &mut engine,
        report(
            0,
            TRACK_FILE,
            ObservationOrigin::Tisb { band: Band::Uat978 },
            0.0,
        ),
    );
    ingest(
        &mut engine,
        report(
            1_000,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            0.0,
        ),
    );

    assert_eq!(
        engine.track_count(),
        2,
        "a single coincidental alignment is not evidence"
    );
}

#[test]
fn a_separated_pair_never_merges_however_long_it_is_watched() {
    let mut engine = engine();
    for round in 0..10 {
        let at = 1_000_000 + round * 1_000_000;
        let drift = round as f64 * 0.0001;
        ingest(
            &mut engine,
            report(
                at,
                TRACK_FILE,
                ObservationOrigin::Tisb { band: Band::Uat978 },
                drift,
            ),
        );
        ingest(
            &mut engine,
            report(
                at + 1_000,
                ICAO,
                ObservationOrigin::AdsbDirect {
                    band: Band::Adsb1090,
                },
                drift + 0.5,
            ),
        );
    }

    assert_eq!(engine.track_count(), 2);
}

#[test]
fn a_vehicle_never_merges_with_an_aircraft() {
    let mut engine = engine();
    ingest(
        &mut engine,
        report(
            0,
            VEHICLE,
            ObservationOrigin::AdsbDirect { band: Band::Uat978 },
            0.0,
        ),
    );
    fly_together(&mut engine, VEHICLE, ICAO, 8);

    assert_eq!(
        engine.track_count(),
        2,
        "a surface vehicle and an aircraft are never one target"
    );
}

#[test]
fn equal_addresses_in_different_namespaces_do_not_merge_on_address() {
    let mut engine = engine();
    let shadow_key = TrackKey::new(AddressNamespace::TisbTrackFile, ICAO.address);

    ingest(
        &mut engine,
        report(
            0,
            shadow_key,
            ObservationOrigin::Tisb { band: Band::Uat978 },
            0.0,
        ),
    );
    ingest(
        &mut engine,
        report(
            1_000,
            ICAO,
            ObservationOrigin::AdsbDirect {
                band: Band::Adsb1090,
            },
            0.5,
        ),
    );

    assert_eq!(
        engine.track_count(),
        2,
        "a shared numeric address is not evidence of one aircraft"
    );
}

#[test]
fn a_merged_track_demotes_when_its_cooperative_source_stops() {
    let mut engine = engine();
    let created = ingest(
        &mut engine,
        report(
            0,
            TRACK_FILE,
            ObservationOrigin::Tisb { band: Band::Uat978 },
            0.0,
        ),
    );
    let id = created[0].id();
    fly_together(&mut engine, TRACK_FILE, ICAO, 6);
    assert_eq!(engine.snapshot_by_id(id).expect("survivor").key, ICAO);

    // The ground source keeps reporting after the cooperative one stops.
    let last_icao = engine
        .snapshot_by_id(id)
        .expect("survivor")
        .last_observed_at_micros;
    let mut deltas = Vec::new();
    for round in 0..6 {
        let at = last_icao + 5_000_000 * (round + 1);
        engine
            .ingest(
                report(
                    at,
                    TRACK_FILE,
                    ObservationOrigin::Tisb { band: Band::Uat978 },
                    0.001 + round as f64 * 0.0001,
                ),
                |delta| deltas.push(delta.clone()),
            )
            .expect("ingest succeeds");
        engine
            .advance_time(at, |delta| deltas.push(delta.clone()))
            .expect("time advance succeeds");
    }

    let snapshot = engine.snapshot_by_id(id).expect("survivor still exists");
    assert_eq!(
        snapshot.key, TRACK_FILE,
        "a silent cooperative identity demotes to the source still reporting"
    );
    assert_eq!(snapshot.id, id, "demotion preserves the track identity");
    assert!(snapshot.identities.contains(ICAO), "history is preserved");
    assert_eq!(snapshot.phase, TrackPhase::Active);
    assert_eq!(
        engine.snapshot(TRACK_FILE).map(|snapshot| snapshot.id),
        Some(id)
    );
}

#[test]
fn a_merge_is_independent_of_which_side_reports_first() {
    fn merged_survivor(icao_first: bool) -> (usize, TrackId) {
        let mut engine = SurveillanceEngine::new(EngineConfig::default(), PRODUCER);
        let (first, second) = if icao_first {
            (ICAO, TRACK_FILE)
        } else {
            (TRACK_FILE, ICAO)
        };
        let created = ingest(
            &mut engine,
            report(
                0,
                first,
                ObservationOrigin::AdsbDirect {
                    band: Band::Adsb1090,
                },
                0.0,
            ),
        );
        fly_together(&mut engine, first, second, 6);
        (engine.track_count(), created[0].id())
    }

    let (icao_first_count, icao_first_id) = merged_survivor(true);
    let (track_file_first_count, track_file_first_id) = merged_survivor(false);

    assert_eq!(icao_first_count, 1);
    assert_eq!(track_file_first_count, 1);
    assert_eq!(
        icao_first_id, track_file_first_id,
        "whichever identity was seen first keeps its track"
    );
}

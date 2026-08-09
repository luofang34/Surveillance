//! Stream contract: a recording drives the engine, and only traffic does.

#![cfg(feature = "std")]
#![allow(clippy::expect_used, clippy::panic)]

#[path = "support/reception_records.rs"]
mod records;

use records::{
    EVEN_POSITION, ODD_POSITION, OTHER_AIRCRAFT, POSITION_ADDRESS, assert_counts_add_up,
    ground_uplink, position_event, recorded, replay, unnormalizable_downlink,
};
use surveillance_aero_link::NormalizeError;
use surveillance_core::{AddressNamespace, FreshnessPolicy, TrackDelta, TrackKey};

/// The requirement this repository had only by construction: a stream that
/// mixes ground uplinks with traffic changes engine state for the traffic
/// alone, and reports no error for the rest.
///
/// The skip path is hot, not an edge case. Close to half of all 978 MHz
/// receptions in real air are ground uplinks.
#[test]
fn a_mixed_stream_admits_traffic_and_skips_ground_uplinks_without_error() {
    let stream = recorded(&[
        ground_uplink(500),
        position_event(1_000, ODD_POSITION),
        ground_uplink(1_500),
        position_event(2_000, EVEN_POSITION),
        ground_uplink(2_500),
    ]);

    let replayed = replay(&stream);
    let counts = replayed.outcome.expect("a ground uplink is not an error");

    assert_eq!(counts.receptions_consumed, 5);
    assert_eq!(counts.observations_produced, 2, "only traffic is observed");
    assert_eq!(
        counts.receptions_skipped_not_traffic, 3,
        "every ground uplink is skipped"
    );
    assert_eq!(counts.receptions_refused, 0);
    assert!(replayed.refusals.is_empty());
    assert_counts_add_up(counts);

    assert_eq!(
        replayed.engine.track_count(),
        1,
        "the uplinks left engine state untouched"
    );
    assert!(matches!(
        replayed.deltas.first(),
        Some(TrackDelta::Created(_))
    ));

    let key = TrackKey::new(AddressNamespace::Icao, POSITION_ADDRESS);
    let snapshot = replayed
        .engine
        .snapshot(key)
        .expect("the target is a track");
    let position = snapshot.position.as_ref().expect("the CPR pair decoded");
    assert!(
        (position.value.latitude_deg - 52.257_202).abs() < 1e-4
            && (position.value.longitude_deg - 3.919_372).abs() < 1e-4,
        "decoded {:?}",
        position.value
    );
}

/// The same contract with nothing else in the stream, so no traffic can mask
/// a failure on the skip path.
#[test]
fn a_stream_of_ground_uplinks_alone_produces_no_observation_and_no_error() {
    let stream = recorded(&[
        ground_uplink(500),
        ground_uplink(1_000),
        ground_uplink(1_500),
    ]);

    let replayed = replay(&stream);
    let counts = replayed.outcome.expect("weather is skipped, not refused");

    assert_eq!(counts.receptions_consumed, 3);
    assert_eq!(counts.observations_produced, 0);
    assert_eq!(counts.receptions_skipped_not_traffic, 3);
    assert_eq!(counts.receptions_refused, 0);
    assert_counts_add_up(counts);
    assert_eq!(replayed.engine.track_count(), 0);
    assert!(
        replayed.deltas.is_empty(),
        "no delta reaches the consumer: {:?}",
        replayed.deltas
    );
}

/// Time moves with the recording. A live runtime coasts and then removes a
/// track that stops transmitting, so a replay of the same length must produce
/// the same deltas. A reader that only ingests produces neither.
#[test]
fn a_recording_that_outlasts_the_freshness_windows_coasts_and_removes() {
    let freshness = FreshnessPolicy::default();
    let last_heard = 2_000;
    // A second aircraft keeps transmitting while the first goes quiet, so the
    // recording carries the arrival times that move the engine's clock through
    // the coasting window and then through the removal window. One single jump
    // past both would expire the track without ever coasting it.
    let stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        position_event(last_heard, EVEN_POSITION),
        position_event(
            last_heard + freshness.coast_after_micros + 1_000_000,
            OTHER_AIRCRAFT,
        ),
        position_event(
            last_heard + freshness.remove_after_micros + 1_000_000,
            OTHER_AIRCRAFT,
        ),
    ]);

    let replayed = replay(&stream);
    replayed.outcome.expect("the recording replays");

    assert!(
        replayed
            .deltas
            .iter()
            .any(|delta| matches!(delta, TrackDelta::Coasting(_))),
        "a track that went quiet must coast: {:?}",
        replayed.deltas
    );
    assert!(
        replayed
            .deltas
            .iter()
            .any(|delta| matches!(delta, TrackDelta::Removed { .. })),
        "a track quiet past the removal window must expire: {:?}",
        replayed.deltas
    );
    let quiet = TrackKey::new(AddressNamespace::Icao, POSITION_ADDRESS);
    assert!(
        replayed.engine.snapshot(quiet).is_none(),
        "the expired track must not still be in the table"
    );
}

/// A reception the adapter cannot normalize is a fault in one reception, not
/// in the recording, so the reader counts it, names it, and reads on.
#[test]
fn a_refused_traffic_reception_is_counted_reported_and_the_stream_continues() {
    let stream = recorded(&[
        position_event(1_000, ODD_POSITION),
        unnormalizable_downlink(1_500),
        position_event(2_000, EVEN_POSITION),
    ]);

    let replayed = replay(&stream);
    let counts = replayed
        .outcome
        .expect("one refused reception does not stop the stream");

    assert_eq!(counts.receptions_consumed, 3);
    assert_eq!(counts.observations_produced, 2);
    assert_eq!(counts.receptions_skipped_not_traffic, 0);
    assert_eq!(counts.receptions_refused, 1);
    assert_counts_add_up(counts);
    assert_eq!(
        replayed.engine.track_count(),
        1,
        "the reception after the refused one still reached the engine"
    );

    // The reason is what separates ordinary refusals from an upstream change
    // that refuses everything. A count alone cannot.
    assert_eq!(
        replayed.refusals,
        vec![(2, NormalizeError::ReservedAddressQualifier { qualifier: 6 })],
        "the caller learns which line refused, and why"
    );
}

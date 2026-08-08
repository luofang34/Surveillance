#![allow(clippy::expect_used, clippy::panic)]

use crate::source::{Band, SourceRef};

use crate::freshness::{FieldProvenance, FieldQuality, TimedField};
use crate::fusion::{FieldUpdate, fuse, link_rank};
use crate::observation::TrafficLink;
use crate::track::AddressNamespace;

const WINDOW: u64 = 10_000;

fn field(value: i32, at: u64, link: TrafficLink) -> TimedField<i32> {
    field_with_quality(value, at, link, FieldQuality::default())
}

fn field_with_quality(
    value: i32,
    at: u64,
    link: TrafficLink,
    quality: FieldQuality,
) -> TimedField<i32> {
    TimedField::new(
        value,
        at,
        quality,
        FieldProvenance::new(link, AddressNamespace::Icao, SourceRef::default()),
    )
}

#[test]
fn the_target_outranks_rebroadcasts() {
    assert!(link_rank(TrafficLink::Adsb1090) > link_rank(TrafficLink::ModeS));
    assert!(link_rank(TrafficLink::Uat978) > link_rank(TrafficLink::ModeS));
    assert!(
        link_rank(TrafficLink::ModeS)
            > link_rank(TrafficLink::AdsR {
                band: Band::Adsb1090
            })
    );
    assert!(
        link_rank(TrafficLink::AdsR {
            band: Band::Adsb1090
        }) > link_rank(TrafficLink::Tisb { band: Band::Uat978 })
    );
}

#[test]
fn an_empty_field_accepts_any_candidate() {
    let mut slot = None;
    assert_eq!(
        fuse(
            &mut slot,
            field(1, 1_000, TrafficLink::Tisb { band: Band::Uat978 }),
            WINDOW
        ),
        FieldUpdate::Changed
    );
    assert_eq!(slot.expect("value applied").value, 1);
}

#[test]
fn a_late_older_observation_never_overwrites_a_newer_value() {
    let mut slot = Some(field(2, 5_000, TrafficLink::Adsb1090));
    assert_eq!(
        fuse(&mut slot, field(1, 4_000, TrafficLink::Adsb1090), WINDOW),
        FieldUpdate::Rejected
    );
    assert_eq!(slot.expect("value retained").value, 2);
}

#[test]
fn a_fresh_rebroadcast_replaces_a_stale_direct_value() {
    let mut slot = Some(field(2, 1_000, TrafficLink::Adsb1090));
    let stale_by_one = 1_000 + WINDOW + 1;
    assert_eq!(
        fuse(
            &mut slot,
            field(7, stale_by_one, TrafficLink::Tisb { band: Band::Uat978 }),
            WINDOW
        ),
        FieldUpdate::Changed
    );
    assert_eq!(slot.expect("value replaced").value, 7);
}

#[test]
fn a_rebroadcast_loses_to_a_fresh_direct_value() {
    let mut slot = Some(field(2, 1_000, TrafficLink::Adsb1090));
    let still_fresh = 1_000 + WINDOW;
    assert_eq!(
        fuse(
            &mut slot,
            field(7, still_fresh, TrafficLink::Tisb { band: Band::Uat978 }),
            WINDOW
        ),
        FieldUpdate::Rejected
    );
    assert_eq!(slot.expect("value retained").value, 2);
}

#[test]
fn a_direct_value_replaces_a_fresh_rebroadcast() {
    let mut slot = Some(field(2, 1_000, TrafficLink::Tisb { band: Band::Uat978 }));
    assert_eq!(
        fuse(&mut slot, field(7, 1_500, TrafficLink::Adsb1090), WINDOW),
        FieldUpdate::Changed
    );
    assert_eq!(slot.expect("value replaced").value, 7);
}

#[test]
fn an_equal_rank_duplicate_refreshes_without_changing_the_value() {
    let mut slot = Some(field(2, 1_000, TrafficLink::Adsb1090));
    assert_eq!(
        fuse(&mut slot, field(2, 1_500, TrafficLink::Adsb1090), WINDOW),
        FieldUpdate::Refreshed
    );
    let applied = slot.expect("value retained");
    assert_eq!(applied.value, 2);
    assert_eq!(applied.observed_at_micros, 1_500);
}

#[test]
fn simultaneous_equal_rank_observations_are_resolved_by_quality() {
    let better = FieldQuality::new(Some(9), None, None, None);
    let worse = FieldQuality::new(Some(6), None, None, None);

    let mut slot = Some(field_with_quality(2, 1_000, TrafficLink::Adsb1090, worse));
    assert_eq!(
        fuse(
            &mut slot,
            field_with_quality(7, 1_000, TrafficLink::Adsb1090, better),
            WINDOW
        ),
        FieldUpdate::Changed
    );
    assert_eq!(slot.expect("better quality wins").value, 7);

    let mut slot = Some(field_with_quality(2, 1_000, TrafficLink::Adsb1090, better));
    assert_eq!(
        fuse(
            &mut slot,
            field_with_quality(7, 1_000, TrafficLink::Adsb1090, worse),
            WINDOW
        ),
        FieldUpdate::Rejected
    );
    assert_eq!(slot.expect("worse quality loses").value, 2);
}

#[test]
fn a_reported_indicator_beats_an_absent_one_at_the_same_instant() {
    let reported = FieldQuality::new(Some(0), None, None, None);
    let absent = FieldQuality::default();

    let mut slot = Some(field_with_quality(2, 1_000, TrafficLink::Adsb1090, absent));
    assert_eq!(
        fuse(
            &mut slot,
            field_with_quality(7, 1_000, TrafficLink::Adsb1090, reported),
            WINDOW
        ),
        FieldUpdate::Changed
    );

    let mut slot = Some(field_with_quality(
        2,
        1_000,
        TrafficLink::Adsb1090,
        reported,
    ));
    assert_eq!(
        fuse(
            &mut slot,
            field_with_quality(7, 1_000, TrafficLink::Adsb1090, absent),
            WINDOW
        ),
        FieldUpdate::Rejected
    );
}

#[test]
fn a_mixed_quality_candidate_does_not_dominate() {
    let current = FieldQuality::new(Some(8), Some(9), None, None);
    let mixed = FieldQuality::new(Some(9), Some(7), None, None);

    let mut slot = Some(field_with_quality(2, 1_000, TrafficLink::Adsb1090, current));
    assert_eq!(
        fuse(
            &mut slot,
            field_with_quality(7, 1_000, TrafficLink::Adsb1090, mixed),
            WINDOW
        ),
        FieldUpdate::Rejected
    );
}

#[test]
fn a_stale_value_yields_to_a_lower_ranked_link_even_at_equal_time() {
    let mut slot = Some(field(2, 1_000, TrafficLink::Adsb1090));
    let far_future = 1_000 + WINDOW * 4;
    assert_eq!(
        fuse(
            &mut slot,
            field(7, far_future, TrafficLink::Tisb { band: Band::Uat978 }),
            WINDOW
        ),
        FieldUpdate::Changed
    );
}

#[test]
fn field_update_reports_meaningful_change() {
    assert!(FieldUpdate::Changed.is_meaningful());
    assert!(!FieldUpdate::Refreshed.is_meaningful());
    assert!(!FieldUpdate::Rejected.is_meaningful());
}

#![allow(clippy::expect_used, clippy::panic)]

use crate::cpr::zone::longitude_zones;
use crate::cpr::{
    AIRBORNE_DEGREES_PER_CYCLE, CprDecodeError, CprFormat, CprFrame, CprFrameError, CprHistory,
    CprPolicy, SURFACE_DEGREES_PER_CYCLE, local, validate_range,
};
use crate::observation::Wgs84Position;

/// Even and odd halves of a published 1090 ES airborne position pair.
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

fn assert_close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() < tolerance,
        "{what}: expected {expected}, decoded {actual}"
    );
}

#[test]
fn accepts_seventeen_bit_coordinates() {
    let frame = CprFrame::new(CprFormat::Even, CprFrame::MAX_ENCODED, 0)
        .expect("boundary values are valid");
    assert_eq!(frame.format, CprFormat::Even);
    assert_eq!(frame.encoded_latitude, CprFrame::MAX_ENCODED);
    assert_eq!(frame.encoded_longitude, 0);
}

#[test]
fn rejects_out_of_range_coordinates() {
    let over = CprFrame::MAX_ENCODED + 1;
    assert_eq!(
        CprFrame::new(CprFormat::Odd, over, 0),
        Err(CprFrameError::EncodedLatitudeRange { value: over })
    );
    assert_eq!(
        CprFrame::new(CprFormat::Odd, 0, over),
        Err(CprFrameError::EncodedLongitudeRange { value: over })
    );
}

#[test]
fn longitude_zone_counts_match_the_definition() {
    assert_eq!(longitude_zones(0.0), 59.0);
    assert_eq!(longitude_zones(87.0), 2.0);
    assert_eq!(longitude_zones(89.5), 1.0);
    assert_eq!(longitude_zones(-89.5), 1.0);
    assert_eq!(longitude_zones(52.2572), 36.0);
    assert_eq!(longitude_zones(-52.2572), 36.0);
}

#[test]
fn a_known_pair_decodes_to_its_published_position() {
    let mut history = CprHistory::default();
    history.record(ODD, 1_000);
    history.record(EVEN, 2_000);

    let position = history
        .decode_global(&CprPolicy::default())
        .expect("a complete fresh pair decodes");
    assert_close(position.latitude_deg, 52.257_202, 1e-5, "latitude");
    assert_close(position.longitude_deg, 3.919_372, 1e-5, "longitude");
}

#[test]
fn the_newest_frame_selects_the_decoded_position() {
    let mut history = CprHistory::default();
    history.record(EVEN, 1_000);
    history.record(ODD, 2_000);

    let position = history
        .decode_global(&CprPolicy::default())
        .expect("a complete fresh pair decodes");
    assert_close(position.latitude_deg, 52.265_780, 1e-5, "latitude");
    assert_close(position.longitude_deg, 3.938_913, 1e-5, "longitude");
}

#[test]
fn an_older_frame_cannot_replace_a_newer_frame_of_the_same_parity() {
    let mut history = CprHistory::default();
    history.record(ODD, 2_000);
    history.record(
        CprFrame {
            format: CprFormat::Odd,
            encoded_latitude: 1,
            encoded_longitude: 1,
        },
        1_000,
    );
    history.record(EVEN, 1_500);

    let position = history
        .decode_global(&CprPolicy::default())
        .expect("the retained new pair decodes");
    assert_close(position.latitude_deg, 52.265_780, 1e-5, "latitude");
    assert_close(position.longitude_deg, 3.938_913, 1e-5, "longitude");
}

#[test]
fn equal_time_frames_have_an_order_independent_tie_break() {
    let alternative = CprFrame {
        format: CprFormat::Odd,
        encoded_latitude: ODD.encoded_latitude - 1,
        encoded_longitude: ODD.encoded_longitude,
    };
    let mut first = CprHistory::default();
    first.record(ODD, 2_000);
    first.record(alternative, 2_000);

    let mut second = CprHistory::default();
    second.record(alternative, 2_000);
    second.record(ODD, 2_000);

    assert_eq!(first, second);
}

#[test]
fn equal_time_pair_selection_does_not_depend_on_arrival_order() {
    let mut even_first = CprHistory::default();
    even_first.record(EVEN, 2_000);
    even_first.record(ODD, 2_000);

    let mut odd_first = CprHistory::default();
    odd_first.record(ODD, 2_000);
    odd_first.record(EVEN, 2_000);

    assert_eq!(
        even_first.decode_global(&CprPolicy::default()),
        odd_first.decode_global(&CprPolicy::default())
    );
}

#[test]
fn a_single_frame_cannot_decode_globally() {
    let mut history = CprHistory::default();
    history.record(EVEN, 1_000);
    assert_eq!(
        history.decode_global(&CprPolicy::default()),
        Err(CprDecodeError::PairIncomplete)
    );

    let mut history = CprHistory::default();
    history.record(ODD, 1_000);
    assert_eq!(
        history.decode_global(&CprPolicy::default()),
        Err(CprDecodeError::PairIncomplete)
    );
}

#[test]
fn a_stale_pair_is_rejected() {
    let policy = CprPolicy::default();
    let mut history = CprHistory::default();
    history.record(ODD, 0);
    history.record(EVEN, policy.max_pair_age_micros + 1);

    assert_eq!(
        history.decode_global(&policy),
        Err(CprDecodeError::PairTooOld {
            age_micros: policy.max_pair_age_micros + 1,
            limit_micros: policy.max_pair_age_micros,
        })
    );
}

#[test]
fn a_pair_at_the_age_limit_still_decodes() {
    let policy = CprPolicy::default();
    let mut history = CprHistory::default();
    history.record(ODD, 0);
    history.record(EVEN, policy.max_pair_age_micros);
    assert!(history.decode_global(&policy).is_ok());
}

#[test]
fn halves_from_different_latitude_zones_are_rejected() {
    // This even half decodes to 14.8282 S while the odd half decodes to
    // 14.8529 S, straddling the boundary between 58 and 57 longitude zones.
    let mut history = CprHistory::default();
    history.record(ODD, 1_000);
    history.record(
        CprFrame {
            format: CprFormat::Even,
            encoded_latitude: 69_290,
            encoded_longitude: 51_372,
        },
        2_000,
    );

    assert_eq!(
        history.decode_global(&CprPolicy::default()),
        Err(CprDecodeError::LatitudeZoneMismatch {
            even_zones: 58,
            odd_zones: 57,
        })
    );
}

#[test]
fn a_pair_decoding_outside_the_geographic_range_is_rejected() {
    let mut history = CprHistory::default();
    history.record(ODD, 1_000);
    history.record(
        CprFrame {
            format: CprFormat::Even,
            encoded_latitude: 10_000,
            encoded_longitude: 51_372,
        },
        2_000,
    );

    let outcome = history.decode_global(&CprPolicy::default());
    assert!(
        matches!(outcome, Err(CprDecodeError::LatitudeOutOfRange { .. })),
        "a decode past the poles must be rejected, got {outcome:?}"
    );
}

#[test]
fn a_replaced_half_pairs_with_the_next_reception() {
    let policy = CprPolicy::default();
    let mut history = CprHistory::default();
    history.record(ODD, 0);
    history.record(EVEN, policy.max_pair_age_micros + 1);
    assert!(history.decode_global(&policy).is_err());

    history.record(ODD, policy.max_pair_age_micros + 2);
    assert!(
        history.decode_global(&policy).is_ok(),
        "a newer counterpart must revive the pair"
    );
}

#[test]
fn a_reference_decode_recovers_the_same_airborne_position() {
    let truth = Wgs84Position {
        latitude_deg: 52.257_202,
        longitude_deg: 3.919_372,
    };
    let nearby = Wgs84Position {
        latitude_deg: 52.0,
        longitude_deg: 3.8,
    };

    let position = local::decode(EVEN, nearby, AIRBORNE_DEGREES_PER_CYCLE);
    assert_close(position.latitude_deg, truth.latitude_deg, 1e-5, "latitude");
    assert_close(
        position.longitude_deg,
        truth.longitude_deg,
        1e-5,
        "longitude",
    );
}

#[test]
fn a_surface_decode_stays_inside_the_reference_quadrant() {
    let reference = Wgs84Position {
        latitude_deg: 40.0,
        longitude_deg: -74.0,
    };
    let position = local::decode(EVEN, reference, SURFACE_DEGREES_PER_CYCLE);

    assert!(
        (position.latitude_deg - reference.latitude_deg).abs() < 1.0,
        "surface latitude {} strayed from the reference",
        position.latitude_deg
    );
    assert!(
        (position.longitude_deg - reference.longitude_deg).abs() < 1.0,
        "surface longitude {} strayed from the reference",
        position.longitude_deg
    );
}

#[test]
fn a_southern_hemisphere_pair_decodes_without_a_negative_zone_index() {
    let mut history = CprHistory::default();
    history.record(
        CprFrame {
            format: CprFormat::Odd,
            encoded_latitude: 131_072 - 74_158,
            encoded_longitude: 50_194,
        },
        1_000,
    );
    history.record(
        CprFrame {
            format: CprFormat::Even,
            encoded_latitude: 131_072 - 93_000,
            encoded_longitude: 51_372,
        },
        2_000,
    );

    let position = history
        .decode_global(&CprPolicy::default())
        .expect("a southern pair decodes");
    assert_close(position.latitude_deg, -52.257_202, 1e-4, "latitude");
    assert!(
        (-90.0..=90.0).contains(&position.latitude_deg),
        "latitude {} is not geographic",
        position.latitude_deg
    );
}

#[test]
fn an_implausible_jump_is_rejected_and_a_reachable_one_is_not() {
    let policy = CprPolicy::default();
    let previous = Wgs84Position {
        latitude_deg: 40.0,
        longitude_deg: -74.0,
    };
    let one_second = 1_000_000;

    let nearby = Wgs84Position {
        latitude_deg: 40.001,
        longitude_deg: -74.0,
    };
    assert!(validate_range(nearby, previous, one_second, &policy).is_ok());

    let far = Wgs84Position {
        latitude_deg: 45.0,
        longitude_deg: -74.0,
    };
    let outcome = validate_range(far, previous, one_second, &policy);
    assert!(
        matches!(outcome, Err(CprDecodeError::ImplausibleRange { .. })),
        "a 500 km jump in one second must be rejected, got {outcome:?}"
    );
}

#[test]
fn a_long_gap_widens_the_plausible_range() {
    let policy = CprPolicy::default();
    let previous = Wgs84Position {
        latitude_deg: 40.0,
        longitude_deg: -74.0,
    };
    let far = Wgs84Position {
        latitude_deg: 45.0,
        longitude_deg: -74.0,
    };

    let one_hour = 3_600_000_000;
    assert!(
        validate_range(far, previous, one_hour, &policy).is_ok(),
        "556 km in an hour is within the speed bound"
    );
}

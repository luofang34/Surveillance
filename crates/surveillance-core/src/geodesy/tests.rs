#![allow(clippy::expect_used, clippy::panic)]

use crate::geodesy::great_circle_distance_m;
use crate::observation::Wgs84Position;

fn at(latitude_deg: f64, longitude_deg: f64) -> Wgs84Position {
    Wgs84Position {
        latitude_deg,
        longitude_deg,
    }
}

#[test]
fn a_position_is_zero_metres_from_itself() {
    let position = at(40.639751, -73.778925);
    assert_eq!(great_circle_distance_m(position, position), 0.0);
}

#[test]
fn one_degree_of_latitude_is_about_sixty_nautical_miles() {
    let metres = great_circle_distance_m(at(0.0, 0.0), at(1.0, 0.0));
    assert!(
        (metres - 111_195.0).abs() < 100.0,
        "one degree of latitude measured {metres} m"
    );
}

#[test]
fn distance_is_symmetric_and_shrinks_with_latitude() {
    let equator = great_circle_distance_m(at(0.0, 0.0), at(0.0, 1.0));
    let reversed = great_circle_distance_m(at(0.0, 1.0), at(0.0, 0.0));
    assert_eq!(equator, reversed);

    let high_latitude = great_circle_distance_m(at(60.0, 0.0), at(60.0, 1.0));
    assert!(
        high_latitude < equator,
        "a degree of longitude at 60 N ({high_latitude} m) must be shorter than at the equator ({equator} m)"
    );
}

#[test]
fn antipodal_positions_are_half_the_circumference_apart() {
    let metres = great_circle_distance_m(at(0.0, 0.0), at(0.0, 180.0));
    assert!(
        (metres - 20_015_086.0).abs() < 1_000.0,
        "antipodal distance measured {metres} m"
    );
}

#[test]
fn a_short_separation_resolves_precisely() {
    let metres = great_circle_distance_m(at(40.0, -74.0), at(40.0, -73.999));
    assert!(
        (metres - 85.2).abs() < 1.0,
        "a thousandth of a degree measured {metres} m"
    );
}

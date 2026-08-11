#![allow(clippy::expect_used, clippy::panic)]

use super::*;

fn declaration(
    version: AdsbVersion,
    supplement_a: bool,
    supplement_c: Option<bool>,
) -> AdsbIntegrityDeclaration {
    AdsbIntegrityDeclaration::new(version, Some(supplement_a), supplement_c, Some(9), Some(3))
        .expect("test declaration is valid")
}

fn airborne(type_code: u8, supplement_b: bool) -> AdsbPositionIntegrity {
    AdsbPositionIntegrity::airborne(type_code, Some(supplement_b))
        .expect("test type code is airborne")
}

fn surface(type_code: u8) -> AdsbPositionIntegrity {
    AdsbPositionIntegrity::surface(type_code).expect("test type code is surface")
}

#[test]
fn do_260a_airborne_codes_follow_the_version_one_table() {
    for (type_code, supplement, expected) in [
        (9, false, 11),
        (10, false, 10),
        (11, true, 9),
        (11, false, 8),
        (12, false, 7),
        (13, true, 6),
        (13, false, 6),
        (14, false, 5),
        (15, false, 4),
        (16, true, 3),
        (16, false, 2),
        (17, false, 1),
        (18, false, 0),
        (20, false, 11),
        (21, false, 10),
        (22, false, 0),
    ] {
        let quality = declaration(AdsbVersion::Do260A, supplement, None)
            .quality_for(airborne(type_code, true));
        assert_eq!(quality.nic, Some(expected), "for type code {type_code}");
    }
}

#[test]
fn do_260b_airborne_codes_require_exact_supplements() {
    for (type_code, supplement_a, supplement_b, expected) in [
        (9, false, false, 11),
        (10, false, false, 10),
        (11, true, true, 9),
        (11, false, false, 8),
        (12, false, false, 7),
        (13, false, true, 6),
        (13, true, true, 6),
        (14, false, false, 5),
        (15, false, false, 4),
        (16, true, true, 3),
        (16, false, false, 2),
        (17, false, false, 1),
        (18, false, false, 0),
        (20, false, false, 11),
        (21, false, false, 10),
        (22, false, false, 0),
    ] {
        let quality = declaration(AdsbVersion::Do260B, supplement_a, None)
            .quality_for(airborne(type_code, supplement_b));
        assert_eq!(quality.nic, Some(expected), "for type code {type_code}");
    }

    let inconsistent =
        declaration(AdsbVersion::Do260B, false, None).quality_for(airborne(11, true));
    assert_eq!(inconsistent.nic, None);
}

#[test]
fn surface_codes_use_the_version_specific_supplements() {
    let v1 = declaration(AdsbVersion::Do260A, true, None).quality_for(surface(7));
    assert_eq!(v1.nic, Some(9));

    for (type_code, supplement_a, supplement_c, expected) in [
        (5, false, false, 11),
        (6, false, false, 10),
        (7, true, false, 9),
        (7, false, false, 8),
        (8, true, true, 7),
        (8, false, true, 6),
        (8, true, false, 6),
        (8, false, false, 0),
    ] {
        let quality = declaration(AdsbVersion::Do260B, supplement_a, Some(supplement_c))
            .quality_for(surface(type_code));
        assert_eq!(quality.nic, Some(expected), "for type code {type_code}");
    }
}

#[test]
fn incomplete_or_inconsistent_inputs_do_not_invent_nic() {
    let no_supplement =
        AdsbIntegrityDeclaration::new(AdsbVersion::Do260B, None, None, Some(8), Some(2))
            .expect("absence is valid");
    let quality = no_supplement.quality_for(airborne(9, false));
    assert_eq!(quality.nic, None);
    assert_eq!(quality.nac_p, Some(8));
    assert_eq!(quality.sil, Some(2));

    let inconsistent = declaration(AdsbVersion::Do260B, true, None).quality_for(airborne(9, false));
    assert_eq!(inconsistent.nic, None);
}

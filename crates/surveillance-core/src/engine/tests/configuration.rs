#![allow(clippy::expect_used, clippy::panic)]

use crate::association::CorrelationPolicy;
use crate::cpr::CprPolicy;
use crate::engine::{EngineConfig, EngineConfigError, SurveillanceEngine};
use crate::freshness::FreshnessPolicy;
use crate::track::ProducerInstanceId;

#[test]
fn default_configuration_is_valid() {
    assert_eq!(EngineConfig::default().validate(), Ok(()));
}

#[test]
fn zero_capacity_and_invalid_lifecycle_are_rejected() {
    let zero_capacity = EngineConfig {
        max_tracks: Some(0),
        ..EngineConfig::default()
    };
    assert_eq!(
        SurveillanceEngine::new(zero_capacity, ProducerInstanceId::new(1))
            .expect_err("zero capacity is invalid"),
        EngineConfigError::ZeroTrackCapacity
    );

    let zero_key_capacity = EngineConfig {
        max_track_keys: Some(0),
        ..EngineConfig::default()
    };
    assert_eq!(
        zero_key_capacity.validate(),
        Err(EngineConfigError::ZeroTrackKeyCapacity)
    );

    let lifecycle = EngineConfig {
        freshness: FreshnessPolicy {
            coast_after_micros: 100,
            remove_after_micros: 100,
            ..FreshnessPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert!(matches!(
        lifecycle.validate(),
        Err(EngineConfigError::LifecycleOrder { .. })
    ));
}

#[test]
fn non_finite_cpr_and_correlation_limits_are_rejected() {
    let cpr = EngineConfig {
        cpr: CprPolicy {
            max_ground_speed_kt: f64::NAN,
            ..CprPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert!(matches!(
        cpr.validate(),
        Err(EngineConfigError::InvalidThreshold {
            name: "max_ground_speed_kt",
            ..
        })
    ));

    let correlation = EngineConfig {
        correlation: CorrelationPolicy {
            max_position_separation_m: f64::INFINITY,
            ..CorrelationPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert!(matches!(
        correlation.validate(),
        Err(EngineConfigError::InvalidThreshold {
            name: "max_position_separation_m",
            ..
        })
    ));
}

#[test]
fn zero_counts_and_inverted_evidence_windows_are_rejected() {
    let zero_count = EngineConfig {
        correlation: CorrelationPolicy {
            associate_after: 0,
            ..CorrelationPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert!(matches!(
        zero_count.validate(),
        Err(EngineConfigError::ZeroAssociationCount { .. })
    ));

    let inverted = EngineConfig {
        correlation: CorrelationPolicy {
            max_evidence_gap_micros: 20,
            max_evidence_window_micros: 10,
            ..CorrelationPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert_eq!(
        inverted.validate(),
        Err(EngineConfigError::EvidenceWindowOrder {
            gap_micros: 20,
            window_micros: 10,
        })
    );
}

#[test]
fn zero_freshness_and_cpr_time_are_rejected() {
    let freshness = EngineConfig {
        freshness: FreshnessPolicy {
            position_fresh_micros: 0,
            ..FreshnessPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert!(matches!(
        freshness.validate(),
        Err(EngineConfigError::ZeroFreshnessWindow {
            name: "position_fresh_micros"
        })
    ));

    let cpr = EngineConfig {
        cpr: CprPolicy {
            max_pair_age_micros: 0,
            ..CprPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert_eq!(cpr.validate(), Err(EngineConfigError::ZeroCprPairAge));

    let extrapolation = EngineConfig {
        freshness: FreshnessPolicy {
            extrapolate_max_micros: 0,
            ..FreshnessPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert!(matches!(
        extrapolation.validate(),
        Err(EngineConfigError::ZeroFreshnessWindow {
            name: "extrapolate_max_micros"
        })
    ));

    let distance = EngineConfig {
        freshness: FreshnessPolicy {
            extrapolate_max_distance_m: 0,
            ..FreshnessPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert!(matches!(
        distance.validate(),
        Err(EngineConfigError::ZeroFreshnessWindow {
            name: "extrapolate_max_distance_m"
        })
    ));
}

#[test]
fn negative_thresholds_and_zero_candidate_capacity_are_rejected() {
    let cpr = EngineConfig {
        cpr: CprPolicy {
            jump_allowance_m: -1.0,
            ..CprPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert!(matches!(
        cpr.validate(),
        Err(EngineConfigError::InvalidThreshold {
            name: "jump_allowance_m",
            ..
        })
    ));

    let altitude = EngineConfig {
        correlation: CorrelationPolicy {
            max_altitude_difference_ft: -1,
            ..CorrelationPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert_eq!(
        altitude.validate(),
        Err(EngineConfigError::NegativeAltitudeDifference { value: -1 })
    );

    let candidates = EngineConfig {
        correlation: CorrelationPolicy {
            max_candidate_peers: 0,
            ..CorrelationPolicy::default()
        },
        ..EngineConfig::default()
    };
    assert_eq!(
        candidates.validate(),
        Err(EngineConfigError::ZeroCandidateCapacity)
    );
}

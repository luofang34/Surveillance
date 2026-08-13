//! Validation of engine policy and capacity settings.

use thiserror::Error;

use crate::engine::EngineConfig;

/// Engine configuration that cannot produce defined tracking behavior.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum EngineConfigError {
    /// Configured track capacity is zero.
    #[error("track capacity must be greater than zero")]
    ZeroTrackCapacity,
    /// Configured track-key capacity is zero.
    #[error("track-key capacity must be greater than zero")]
    ZeroTrackKeyCapacity,
    /// One freshness window is zero.
    #[error("freshness window {name} must be greater than zero")]
    ZeroFreshnessWindow {
        /// Name of the invalid window.
        name: &'static str,
    },
    /// Coasting does not occur before removal.
    #[error(
        "coasting threshold {coast_after_micros} must be less than removal threshold {remove_after_micros}"
    )]
    LifecycleOrder {
        /// Coasting threshold.
        coast_after_micros: u64,
        /// Removal threshold.
        remove_after_micros: u64,
    },
    /// CPR pair age is zero.
    #[error("CPR pair age must be greater than zero")]
    ZeroCprPairAge,
    /// One floating-point threshold is not finite or has an invalid sign.
    #[error("configuration value {name} is invalid: {value}")]
    InvalidThreshold {
        /// Name of the invalid value.
        name: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// One association count is zero.
    #[error("association count {name} must be greater than zero")]
    ZeroAssociationCount {
        /// Name of the invalid count.
        name: &'static str,
    },
    /// Evidence gap or window is zero.
    #[error("association time {name} must be greater than zero")]
    ZeroAssociationTime {
        /// Name of the invalid time.
        name: &'static str,
    },
    /// Evidence gap is larger than its complete window.
    #[error("evidence gap {gap_micros} exceeds evidence window {window_micros}")]
    EvidenceWindowOrder {
        /// Maximum gap.
        gap_micros: u64,
        /// Maximum complete window.
        window_micros: u64,
    },
    /// Candidate evidence capacity is zero.
    #[error("association candidate capacity must be greater than zero")]
    ZeroCandidateCapacity,
    /// Altitude difference is negative.
    #[error("altitude difference {value} ft must not be negative")]
    NegativeAltitudeDifference {
        /// Rejected difference.
        value: i32,
    },
}

impl EngineConfig {
    /// Check all engine policy invariants.
    pub fn validate(&self) -> Result<(), EngineConfigError> {
        if self.max_tracks == Some(0) {
            return Err(EngineConfigError::ZeroTrackCapacity);
        }
        if self.max_track_keys == Some(0) {
            return Err(EngineConfigError::ZeroTrackKeyCapacity);
        }
        validate_freshness(self)?;
        validate_cpr(self)?;
        validate_correlation(self)
    }
}

fn validate_freshness(config: &EngineConfig) -> Result<(), EngineConfigError> {
    let policy = config.freshness;
    for (name, value) in [
        ("position_fresh_micros", policy.position_fresh_micros),
        ("velocity_fresh_micros", policy.velocity_fresh_micros),
        ("identity_fresh_micros", policy.identity_fresh_micros),
        ("integrity_fresh_micros", policy.integrity_fresh_micros),
        ("coast_after_micros", policy.coast_after_micros),
        ("remove_after_micros", policy.remove_after_micros),
        ("extrapolate_max_micros", policy.extrapolate_max_micros),
        (
            "extrapolate_max_distance_m",
            policy.extrapolate_max_distance_m,
        ),
    ] {
        if value == 0 {
            return Err(EngineConfigError::ZeroFreshnessWindow { name });
        }
    }
    if policy.coast_after_micros >= policy.remove_after_micros {
        return Err(EngineConfigError::LifecycleOrder {
            coast_after_micros: policy.coast_after_micros,
            remove_after_micros: policy.remove_after_micros,
        });
    }
    Ok(())
}

fn validate_cpr(config: &EngineConfig) -> Result<(), EngineConfigError> {
    if config.cpr.max_pair_age_micros == 0 {
        return Err(EngineConfigError::ZeroCprPairAge);
    }
    validate_positive("max_ground_speed_kt", config.cpr.max_ground_speed_kt)?;
    validate_nonnegative("jump_allowance_m", config.cpr.jump_allowance_m)
}

fn validate_correlation(config: &EngineConfig) -> Result<(), EngineConfigError> {
    let policy = config.correlation;
    validate_positive(
        "max_position_separation_m",
        policy.max_position_separation_m,
    )?;
    validate_nonnegative(
        "max_ground_speed_difference_kt",
        policy.max_ground_speed_difference_kt,
    )?;
    if policy.max_altitude_difference_ft < 0 {
        return Err(EngineConfigError::NegativeAltitudeDifference {
            value: policy.max_altitude_difference_ft,
        });
    }
    for (name, value) in [
        ("associate_after", policy.associate_after),
        ("dissociate_after", policy.dissociate_after),
    ] {
        if value == 0 {
            return Err(EngineConfigError::ZeroAssociationCount { name });
        }
    }
    validate_evidence_limits(config)
}

fn validate_evidence_limits(config: &EngineConfig) -> Result<(), EngineConfigError> {
    let policy = config.correlation;
    for (name, value) in [
        ("max_evidence_gap_micros", policy.max_evidence_gap_micros),
        (
            "max_evidence_window_micros",
            policy.max_evidence_window_micros,
        ),
        ("demote_after_micros", policy.demote_after_micros),
    ] {
        if value == 0 {
            return Err(EngineConfigError::ZeroAssociationTime { name });
        }
    }
    if policy.max_evidence_gap_micros > policy.max_evidence_window_micros {
        return Err(EngineConfigError::EvidenceWindowOrder {
            gap_micros: policy.max_evidence_gap_micros,
            window_micros: policy.max_evidence_window_micros,
        });
    }
    if policy.max_candidate_peers == 0 {
        return Err(EngineConfigError::ZeroCandidateCapacity);
    }
    Ok(())
}

fn validate_positive(name: &'static str, value: f64) -> Result<(), EngineConfigError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(EngineConfigError::InvalidThreshold { name, value });
    }
    Ok(())
}

fn validate_nonnegative(name: &'static str, value: f64) -> Result<(), EngineConfigError> {
    if !value.is_finite() || value < 0.0 {
        return Err(EngineConfigError::InvalidThreshold { name, value });
    }
    Ok(())
}

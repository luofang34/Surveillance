//! Validation of canonical observation values.

use thiserror::Error;

use crate::cpr::CprFrameError;
use crate::observation::{
    AdsbPositionKind, ObservationTime, PositionObservation, TimeBasis, TrafficObservation,
    VelocityObservation, Wgs84Position,
};

/// A WGS84 position that cannot identify a geographic point.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum InvalidPosition {
    /// Latitude is not finite.
    #[error("latitude {value} is not finite")]
    LatitudeNotFinite {
        /// Rejected latitude.
        value: f64,
    },
    /// Longitude is not finite.
    #[error("longitude {value} is not finite")]
    LongitudeNotFinite {
        /// Rejected longitude.
        value: f64,
    },
    /// Latitude is outside the WGS84 range.
    #[error("latitude {value} is outside -90 through 90 degrees")]
    LatitudeRange {
        /// Rejected latitude.
        value: f64,
    },
    /// Longitude is outside the WGS84 range.
    #[error("longitude {value} is outside -180 through 180 degrees")]
    LongitudeRange {
        /// Rejected longitude.
        value: f64,
    },
}

impl Wgs84Position {
    /// Check that this value identifies a WGS84 geographic point.
    pub fn validate(self) -> Result<(), InvalidPosition> {
        if !self.latitude_deg.is_finite() {
            return Err(InvalidPosition::LatitudeNotFinite {
                value: self.latitude_deg,
            });
        }
        if !self.longitude_deg.is_finite() {
            return Err(InvalidPosition::LongitudeNotFinite {
                value: self.longitude_deg,
            });
        }
        if !(-90.0..=90.0).contains(&self.latitude_deg) {
            return Err(InvalidPosition::LatitudeRange {
                value: self.latitude_deg,
            });
        }
        if !(-180.0..=180.0).contains(&self.longitude_deg) {
            return Err(InvalidPosition::LongitudeRange {
                value: self.longitude_deg,
            });
        }
        Ok(())
    }
}

/// A velocity component that the engine cannot compare safely.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum InvalidVelocity {
    /// Ground speed is negative or not finite.
    #[error("ground speed {value} kt is negative or not finite")]
    GroundSpeed {
        /// Rejected speed.
        value: f64,
    },
    /// True track angle is outside one turn or is not finite.
    #[error("true track angle {value} degrees is outside 0 through 360")]
    TrackAngle {
        /// Rejected angle.
        value: f64,
    },
    /// Airspeed is negative or not finite.
    #[error("airspeed {value} kt is negative or not finite")]
    Airspeed {
        /// Rejected speed.
        value: f64,
    },
    /// Heading is outside one turn or is not finite.
    #[error("heading {value} degrees is outside 0 through 360")]
    Heading {
        /// Rejected angle.
        value: f64,
    },
}

impl VelocityObservation {
    /// Check all supplied floating-point movement components.
    pub fn validate(self) -> Result<(), InvalidVelocity> {
        validate_nonnegative(self.ground_speed_kt, |value| InvalidVelocity::GroundSpeed {
            value,
        })?;
        validate_angle(self.track_angle_deg_true, |value| {
            InvalidVelocity::TrackAngle { value }
        })?;
        validate_nonnegative(self.airspeed_kt, |value| InvalidVelocity::Airspeed {
            value,
        })?;
        validate_angle(self.heading_deg, |value| InvalidVelocity::Heading { value })
    }
}

fn validate_nonnegative(
    value: Option<f64>,
    error: impl FnOnce(f64) -> InvalidVelocity,
) -> Result<(), InvalidVelocity> {
    if let Some(value) = value
        && (!value.is_finite() || value < 0.0)
    {
        return Err(error(value));
    }
    Ok(())
}

fn validate_angle(
    value: Option<f64>,
    error: impl FnOnce(f64) -> InvalidVelocity,
) -> Result<(), InvalidVelocity> {
    if let Some(value) = value
        && (!value.is_finite() || !(0.0..360.0).contains(&value))
    {
        return Err(error(value));
    }
    Ok(())
}

/// Name of one integrity or accuracy indicator.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityIndicator {
    /// Navigation integrity category.
    Nic,
    /// Navigation accuracy category for position.
    NacP,
    /// Navigation accuracy category for velocity.
    NacV,
    /// Source integrity level.
    Sil,
}

/// A canonical observation that violates an engine input invariant.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum InvalidObservation {
    /// Observation time is after reception time.
    #[error("observation time {observed_at_micros} is after reception time {received_at_micros}")]
    ObservationAfterReception {
        /// Rejected observation time.
        observed_at_micros: u64,
        /// Reception time.
        received_at_micros: u64,
    },
    /// A local or unknown time basis does not use reception time.
    #[error("time basis {basis:?} requires observation time to equal reception time")]
    TimeBasisMismatch {
        /// Rejected time basis.
        basis: TimeBasis,
    },
    /// Address exceeds the canonical 24-bit range.
    #[error("track address {address:#010x} exceeds 24 bits")]
    AddressRange {
        /// Rejected address.
        address: u32,
    },
    /// Decoded position is invalid.
    #[error("decoded position is invalid")]
    Position {
        /// Position validation failure.
        #[source]
        source: InvalidPosition,
    },
    /// CPR content is invalid.
    #[error("CPR frame is invalid")]
    CprFrame {
        /// CPR validation failure.
        #[source]
        source: CprFrameError,
    },
    /// Velocity content is invalid.
    #[error("velocity is invalid")]
    Velocity {
        /// Velocity validation failure.
        #[source]
        source: InvalidVelocity,
    },
    /// Quality indicator is outside its defined range.
    #[error("quality indicator {indicator:?} value {value} exceeds {maximum}")]
    QualityRange {
        /// Rejected indicator.
        indicator: QualityIndicator,
        /// Rejected value.
        value: u8,
        /// Largest defined value.
        maximum: u8,
    },
    /// ADS-B position integrity metadata has no position to qualify.
    #[error("ADS-B position integrity metadata requires a position")]
    AdsbIntegrityWithoutPosition,
    /// ADS-B type-code metadata cannot qualify a decoded position.
    #[error("ADS-B position integrity metadata cannot qualify a decoded position")]
    AdsbIntegrityWithDecodedPosition,
    /// ADS-B integrity metadata names a different position class.
    #[error("ADS-B integrity class {integrity:?} does not match position class {position:?}")]
    AdsbIntegrityPositionMismatch {
        /// Position class in the metadata.
        integrity: AdsbPositionKind,
        /// Position class in the observation.
        position: AdsbPositionKind,
    },
}

impl TrafficObservation {
    /// Check all canonical values before the engine mutates track state.
    pub fn validate(&self) -> Result<(), InvalidObservation> {
        validate_time(self.time)?;
        if self.key.address > 0x00ff_ffff {
            return Err(InvalidObservation::AddressRange {
                address: self.key.address,
            });
        }
        if let Some(position) = self.position {
            validate_position(position)?;
        }
        validate_adsb_position_integrity(self)?;
        if let Some(velocity) = self.velocity {
            velocity
                .validate()
                .map_err(|source| InvalidObservation::Velocity { source })?;
        }
        validate_quality(self.quality.nic, QualityIndicator::Nic, 11)?;
        validate_quality(self.quality.nac_p, QualityIndicator::NacP, 11)?;
        validate_quality(self.quality.nac_v, QualityIndicator::NacV, 4)?;
        validate_quality(self.quality.sil, QualityIndicator::Sil, 3)
    }
}

fn validate_adsb_position_integrity(
    observation: &TrafficObservation,
) -> Result<(), InvalidObservation> {
    let Some(integrity) = observation.adsb_position_integrity else {
        return Ok(());
    };
    let Some(position) = observation.position else {
        return Err(InvalidObservation::AdsbIntegrityWithoutPosition);
    };
    let position_kind = match position {
        PositionObservation::CprAirborne(_) => AdsbPositionKind::Airborne,
        PositionObservation::CprSurface(_) => AdsbPositionKind::Surface,
        PositionObservation::Decoded(_) => {
            return Err(InvalidObservation::AdsbIntegrityWithDecodedPosition);
        }
    };
    if integrity.kind() != position_kind {
        return Err(InvalidObservation::AdsbIntegrityPositionMismatch {
            integrity: integrity.kind(),
            position: position_kind,
        });
    }
    Ok(())
}

fn validate_time(time: ObservationTime) -> Result<(), InvalidObservation> {
    if time.observed_at_micros > time.received_at_micros {
        return Err(InvalidObservation::ObservationAfterReception {
            observed_at_micros: time.observed_at_micros,
            received_at_micros: time.received_at_micros,
        });
    }
    if matches!(
        time.basis,
        TimeBasis::LocalIngress | TimeBasis::UnknownSourceTime
    ) && time.observed_at_micros != time.received_at_micros
    {
        return Err(InvalidObservation::TimeBasisMismatch { basis: time.basis });
    }
    Ok(())
}

fn validate_position(position: PositionObservation) -> Result<(), InvalidObservation> {
    match position {
        PositionObservation::Decoded(position) => position
            .validate()
            .map_err(|source| InvalidObservation::Position { source }),
        PositionObservation::CprAirborne(frame) | PositionObservation::CprSurface(frame) => frame
            .validate()
            .map_err(|source| InvalidObservation::CprFrame { source }),
    }
}

fn validate_quality(
    value: Option<u8>,
    indicator: QualityIndicator,
    maximum: u8,
) -> Result<(), InvalidObservation> {
    if let Some(value) = value
        && value > maximum
    {
        return Err(InvalidObservation::QualityRange {
            indicator,
            value,
            maximum,
        });
    }
    Ok(())
}

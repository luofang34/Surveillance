//! Encoding of individual GDL 90 Traffic Report fields.

use surveillance_core::{
    Callsign, EmitterCategory, HeadingReference, TimedField, VelocityObservation,
};

use crate::report::TrafficReportError;

pub(super) fn integrity_code(
    field: &'static str,
    value: Option<u8>,
) -> Result<u8, TrafficReportError> {
    match value {
        Some(value @ 0..=11) => Ok(value),
        None => Ok(0),
        Some(value) => Err(TrafficReportError::Integrity { field, value }),
    }
}

pub(super) fn position_code(value: f64, latitude: bool) -> Result<i32, TrafficReportError> {
    let limit = if latitude { 90.0 } else { 180.0 };
    if !value.is_finite() || !(-limit..=limit).contains(&value) {
        return Err(TrafficReportError::Position {
            position: if latitude {
                surveillance_core::Wgs84Position {
                    latitude_deg: value,
                    longitude_deg: 0.0,
                }
            } else {
                surveillance_core::Wgs84Position {
                    latitude_deg: 0.0,
                    longitude_deg: value,
                }
            },
        });
    }
    let scaled = libm::round(value * 8_388_608.0 / 180.0);
    Ok(if scaled >= 8_388_608.0 {
        8_388_607
    } else {
        scaled as i32
    })
}

pub(super) fn altitude_code(pressure_altitude_ft: Option<i32>) -> u16 {
    let Some(feet) = pressure_altitude_ft else {
        return 0x0fff;
    };
    if !(-1_000..=101_350).contains(&feet) {
        return 0x0fff;
    }
    ((i64::from(feet) + 1_012) / 25) as u16
}

pub(super) fn horizontal_velocity(
    velocity: Option<&TimedField<VelocityObservation>>,
) -> Result<u16, TrafficReportError> {
    let Some(value) = velocity.and_then(|field| field.value.ground_speed_kt) else {
        return Ok(0x0fff);
    };
    validate_movement("ground_speed_kt", value, false)?;
    Ok(libm::round(value).min(4094.0) as u16)
}

pub(super) fn vertical_velocity(velocity: Option<&TimedField<VelocityObservation>>) -> u16 {
    let Some(value) = velocity.and_then(|field| field.value.vertical_rate_fpm) else {
        return 0x0800;
    };
    if value > 32_576 {
        return 0x01fe;
    }
    if value < -32_576 {
        return 0x0e02;
    }
    let units = if value >= 0 {
        (value + 32) / 64
    } else {
        -(((value.unsigned_abs() + 32) / 64) as i32)
    };
    (units & 0x0fff) as u16
}

pub(super) fn direction(
    velocity: Option<&TimedField<VelocityObservation>>,
) -> Result<u8, TrafficReportError> {
    let Some(velocity) = velocity else {
        return Ok(0);
    };
    let angle = if let Some(track) = velocity.value.track_angle_deg_true {
        Some(("track_angle_deg_true", track))
    } else if velocity.value.heading_reference.is_some() {
        velocity
            .value
            .heading_deg
            .map(|heading| ("heading_deg", heading))
    } else {
        None
    };
    let Some((field, value)) = angle else {
        return Ok(0);
    };
    validate_movement(field, value, true)?;
    Ok((libm::round(value * 256.0 / 360.0) as u16 % 256) as u8)
}

pub(super) fn direction_kind(velocity: Option<&TimedField<VelocityObservation>>) -> u8 {
    let Some(velocity) = velocity else {
        return 0;
    };
    if velocity.value.track_angle_deg_true.is_some() {
        return 1;
    }
    match (velocity.value.heading_deg, velocity.value.heading_reference) {
        (Some(_), Some(HeadingReference::MagneticNorth)) => 2,
        (Some(_), Some(HeadingReference::TrueNorth)) => 3,
        _ => 0,
    }
}

pub(super) fn emitter_category(field: Option<&TimedField<EmitterCategory>>) -> u8 {
    match field.map(|value| value.value) {
        Some(EmitterCategory::LightAircraft) => 1,
        Some(EmitterCategory::SmallAircraft) => 2,
        Some(EmitterCategory::LargeAircraft) => 3,
        Some(EmitterCategory::HighVortexAircraft) => 4,
        Some(EmitterCategory::HeavyAircraft) => 5,
        Some(EmitterCategory::HighPerformanceAircraft) => 6,
        Some(EmitterCategory::Rotorcraft) => 7,
        Some(EmitterCategory::Glider) => 9,
        Some(EmitterCategory::LighterThanAir) => 10,
        Some(EmitterCategory::Parachutist) => 11,
        Some(EmitterCategory::Ultralight) => 12,
        Some(EmitterCategory::UncrewedAircraft) => 14,
        Some(EmitterCategory::SpaceVehicle) => 15,
        Some(EmitterCategory::SurfaceEmergencyVehicle) => 17,
        Some(EmitterCategory::SurfaceServiceVehicle) => 18,
        Some(EmitterCategory::PointObstacle) => 19,
        Some(EmitterCategory::ClusterObstacle) => 20,
        Some(EmitterCategory::LineObstacle) => 21,
        None | Some(_) => 0,
    }
}

pub(super) fn callsign(
    field: Option<&TimedField<Callsign>>,
) -> Result<[u8; 8], TrafficReportError> {
    let mut output = [b' '; 8];
    let Some(callsign) = field else {
        return Ok(output);
    };
    let bytes = callsign.value.text.as_bytes();
    if bytes.len() > output.len() {
        return Err(TrafficReportError::CallsignLength {
            actual: bytes.len(),
        });
    }
    let mut padding_started = false;
    for (index, value) in bytes.iter().copied().enumerate() {
        if value == b' ' {
            padding_started = true;
        } else if padding_started || !(value.is_ascii_uppercase() || value.is_ascii_digit()) {
            return Err(TrafficReportError::CallsignCharacter { index, value });
        }
        output[index] = value;
    }
    Ok(output)
}

fn validate_movement(
    field: &'static str,
    value: f64,
    angle: bool,
) -> Result<(), TrafficReportError> {
    let invalid = !value.is_finite() || value < 0.0 || (angle && value >= 360.0);
    if invalid {
        Err(TrafficReportError::Movement { field, value })
    } else {
        Ok(())
    }
}

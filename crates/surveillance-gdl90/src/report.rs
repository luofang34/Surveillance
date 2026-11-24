//! Traffic Report assembly and caller-facing errors.

use surveillance_core::{
    AddressNamespace, AirGroundState, EmergencyState, ObservationOrigin, TrackKey, TrackSnapshot,
    Wgs84Position,
};
use thiserror::Error;

use crate::value::{
    altitude_code, callsign, direction, emitter_category, horizontal_velocity, integrity_code,
    position_code, vertical_velocity,
};

/// Number of bytes in one unframed GDL 90 Traffic Report message.
pub const GDL90_TRAFFIC_REPORT_LEN: usize = 28;

/// Caller-supplied result of its traffic-alert policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrafficAlertStatus {
    /// No traffic alert applies to the target.
    NoAlert,
    /// The caller's alerting service selected the target.
    TrafficAlert,
}

/// Result of a successful Traffic Report encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrafficReportEncoding {
    /// Number of bytes written to the caller's buffer.
    pub bytes_written: usize,
    /// Whether core labeled this track as an ownship rebroadcast shadow.
    pub ownship_shadow: bool,
}

/// Failure mapping a snapshot to GDL 90.
#[non_exhaustive]
#[derive(Clone, Debug, Error, PartialEq)]
pub enum TrafficReportError {
    /// Caller storage cannot hold one complete report.
    #[error("traffic report needs {required} bytes, but the buffer has {available}")]
    BufferCapacity {
        /// Required byte count.
        required: usize,
        /// Available byte count.
        available: usize,
    },
    /// Track address exceeds the GDL 90 participant-address field.
    #[error("participant address {address:#010x} exceeds 24 bits")]
    AddressRange {
        /// Rejected address.
        address: u32,
    },
    /// The track key does not identify one supported GDL 90 address type.
    #[error("track key {key:?} has no supported GDL 90 address type")]
    AddressTypeUnavailable {
        /// Key that cannot select a GDL 90 address type.
        key: TrackKey,
    },
    /// No air or ground state is available for an aircraft address.
    #[error("GDL 90 has no unavailable encoding for air or ground state")]
    AirGroundUnavailable,
    /// A reserved air or ground state has no GDL 90 mapping.
    #[error("reserved air or ground state has no GDL 90 mapping")]
    ReservedAirGround,
    /// No emergency state is available.
    #[error("GDL 90 has no unavailable encoding for emergency state")]
    EmergencyUnavailable,
    /// Position state says that a position exists, but the value is absent.
    #[error("position freshness state and position value do not agree")]
    PositionStateMismatch,
    /// Position content is outside the WGS84 range.
    #[error("position cannot be encoded as GDL 90 semicircles")]
    Position {
        /// Rejected position.
        position: Wgs84Position,
    },
    /// A velocity or angle is negative or not finite.
    #[error("movement value {field} is invalid: {value}")]
    Movement {
        /// Name of the rejected field.
        field: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// A position quality code is outside the GDL 90 range.
    #[error("position quality value {field}={value} is not valid GDL 90")]
    Integrity {
        /// Name of the rejected quality field.
        field: &'static str,
        /// Rejected quality code.
        value: u8,
    },
    /// Callsign exceeds the eight-byte field.
    #[error("callsign has {actual} bytes, but GDL 90 allows 8")]
    CallsignLength {
        /// Rejected byte count.
        actual: usize,
    },
    /// Callsign contains a character GDL 90 cannot represent.
    #[error("callsign byte {value:#04x} at index {index} is not valid GDL 90 text")]
    CallsignCharacter {
        /// Zero-based byte index.
        index: usize,
        /// Rejected byte.
        value: u8,
    },
}

/// Encode one unframed GDL 90 Traffic Report.
///
/// The caller supplies traffic-alert policy. The returned ownship-shadow flag
/// lets the gateway suppress or route a report without this adapter choosing.
/// The output buffer does not change when encoding fails.
///
/// # Errors
///
/// Returns [`TrafficReportError`] for insufficient storage or a value that
/// GDL 90 cannot represent without inventing information.
pub fn encode_traffic_report(
    snapshot: &TrackSnapshot,
    alert: TrafficAlertStatus,
    output: &mut [u8],
) -> Result<TrafficReportEncoding, TrafficReportError> {
    if output.len() < GDL90_TRAFFIC_REPORT_LEN {
        return Err(TrafficReportError::BufferCapacity {
            required: GDL90_TRAFFIC_REPORT_LEN,
            available: output.len(),
        });
    }
    let mut report = [0_u8; GDL90_TRAFFIC_REPORT_LEN];
    encode(snapshot, alert, &mut report)?;
    output[..GDL90_TRAFFIC_REPORT_LEN].copy_from_slice(&report);
    Ok(TrafficReportEncoding {
        bytes_written: GDL90_TRAFFIC_REPORT_LEN,
        ownship_shadow: snapshot.ownship_shadow,
    })
}

fn encode(
    snapshot: &TrackSnapshot,
    alert: TrafficAlertStatus,
    report: &mut [u8; GDL90_TRAFFIC_REPORT_LEN],
) -> Result<(), TrafficReportError> {
    if snapshot.key.address > 0x00ff_ffff {
        return Err(TrafficReportError::AddressRange {
            address: snapshot.key.address,
        });
    }
    report[0] = 20;
    report[1] = (alert_code(alert) << 4) | address_type(snapshot)?;
    report[2..5].copy_from_slice(&snapshot.key.address.to_be_bytes()[1..]);

    let position = match snapshot.position_status {
        surveillance_core::PositionStatus::Unavailable if snapshot.position.is_none() => None,
        surveillance_core::PositionStatus::Unavailable => {
            return Err(TrafficReportError::PositionStateMismatch);
        }
        surveillance_core::PositionStatus::Current | surveillance_core::PositionStatus::Stale => {
            Some(
                snapshot
                    .position
                    .as_ref()
                    .ok_or(TrafficReportError::PositionStateMismatch)?,
            )
        }
        _ => return Err(TrafficReportError::PositionStateMismatch),
    };
    let (latitude, longitude, nic, nac_p) = if let Some(position) = position {
        (
            position_code(position.value.latitude_deg, true)?,
            position_code(position.value.longitude_deg, false)?,
            integrity_code("nic", position.quality.nic)?,
            integrity_code("nac_p", position.quality.nac_p)?,
        )
    } else {
        (0, 0, 0, 0)
    };
    write_24(&mut report[5..8], latitude);
    write_24(&mut report[8..11], longitude);

    let altitude = altitude_code(
        snapshot
            .pressure_altitude_ft
            .as_ref()
            .map(|field| field.value),
    );
    let miscellaneous = miscellaneous(snapshot)?;
    report[11] = (altitude >> 4) as u8;
    report[12] = ((altitude & 0x0f) as u8) << 4 | miscellaneous;
    report[13] = (nic << 4) | nac_p;

    let horizontal = horizontal_velocity(snapshot.velocity.as_ref())?;
    let vertical = vertical_velocity(snapshot.velocity.as_ref());
    report[14] = (horizontal >> 4) as u8;
    report[15] = ((horizontal & 0x0f) as u8) << 4 | ((vertical >> 8) as u8 & 0x0f);
    report[16] = vertical as u8;
    report[17] = direction(snapshot.velocity.as_ref())?;
    report[18] = emitter_category(snapshot.emitter_category.as_ref());
    report[19..27].copy_from_slice(&callsign(snapshot.callsign.as_ref())?);
    report[27] = emergency(snapshot.emergency.as_ref())? << 4;
    Ok(())
}

fn alert_code(status: TrafficAlertStatus) -> u8 {
    match status {
        TrafficAlertStatus::NoAlert => 0,
        TrafficAlertStatus::TrafficAlert => 1,
    }
}

fn address_type(snapshot: &TrackSnapshot) -> Result<u8, TrafficReportError> {
    let code = match snapshot.key.namespace {
        AddressNamespace::Icao => match representative_origin(snapshot) {
            Some(ObservationOrigin::AdsbDirect { .. } | ObservationOrigin::AdsR { .. }) => 0,
            Some(ObservationOrigin::Tisb { .. }) => 2,
            _ => {
                return Err(TrafficReportError::AddressTypeUnavailable { key: snapshot.key });
            }
        },
        AddressNamespace::SelfAssigned => 1,
        AddressNamespace::TisbTrackFile | AddressNamespace::TisbModeATrackFile => 3,
        AddressNamespace::SurfaceVehicle => 4,
        AddressNamespace::FixedBeacon => 5,
        AddressNamespace::AdsbNonIcao => {
            return Err(TrafficReportError::AddressTypeUnavailable { key: snapshot.key });
        }
        _ => return Err(TrafficReportError::AddressTypeUnavailable { key: snapshot.key }),
    };
    Ok(code)
}

fn representative_origin(snapshot: &TrackSnapshot) -> Option<ObservationOrigin> {
    snapshot
        .position
        .as_ref()
        .map(|field| field.provenance.origin)
        .or_else(|| {
            snapshot
                .pressure_altitude_ft
                .as_ref()
                .map(|field| field.provenance.origin)
        })
        .or_else(|| {
            snapshot
                .geometric_altitude_ft
                .as_ref()
                .map(|field| field.provenance.origin)
        })
        .or_else(|| {
            snapshot
                .velocity
                .as_ref()
                .map(|field| field.provenance.origin)
        })
        .or_else(|| {
            snapshot
                .callsign
                .as_ref()
                .map(|field| field.provenance.origin)
        })
        .or_else(|| {
            snapshot
                .squawk
                .as_ref()
                .map(|field| field.provenance.origin)
        })
        .or_else(|| {
            snapshot
                .air_ground
                .as_ref()
                .map(|field| field.provenance.origin)
        })
        .or_else(|| {
            snapshot
                .emergency
                .as_ref()
                .map(|field| field.provenance.origin)
        })
        .or_else(|| {
            snapshot
                .emitter_category
                .as_ref()
                .map(|field| field.provenance.origin)
        })
}

fn miscellaneous(snapshot: &TrackSnapshot) -> Result<u8, TrafficReportError> {
    let airborne = match snapshot.air_ground.as_ref().map(|field| field.value) {
        Some(AirGroundState::Airborne | AirGroundState::Subsonic | AirGroundState::Supersonic) => {
            true
        }
        Some(AirGroundState::Ground) => false,
        Some(AirGroundState::Reserved) => return Err(TrafficReportError::ReservedAirGround),
        None if matches!(
            snapshot.key.namespace,
            AddressNamespace::SurfaceVehicle | AddressNamespace::FixedBeacon
        ) =>
        {
            false
        }
        None => return Err(TrafficReportError::AirGroundUnavailable),
        Some(_) => return Err(TrafficReportError::ReservedAirGround),
    };
    let extrapolated = snapshot.phase == surveillance_core::TrackPhase::Coasting
        || snapshot.position_status == surveillance_core::PositionStatus::Stale;
    Ok((u8::from(airborne) << 3)
        | (u8::from(extrapolated) << 2)
        | crate::value::direction_kind(snapshot.velocity.as_ref()))
}

fn emergency(
    field: Option<&surveillance_core::TimedField<EmergencyState>>,
) -> Result<u8, TrafficReportError> {
    match field.map(|value| value.value) {
        Some(EmergencyState::None) => Ok(0),
        Some(EmergencyState::General) => Ok(1),
        Some(EmergencyState::Medical) => Ok(2),
        Some(EmergencyState::MinimumFuel) => Ok(3),
        Some(EmergencyState::NoCommunication) => Ok(4),
        Some(EmergencyState::UnlawfulInterference) => Ok(5),
        Some(EmergencyState::DownedAircraft) => Ok(6),
        Some(EmergencyState::Reserved) => Ok(7),
        None => Err(TrafficReportError::EmergencyUnavailable),
        Some(_) => Ok(7),
    }
}

fn write_24(output: &mut [u8], value: i32) {
    let bytes = value.to_be_bytes();
    output.copy_from_slice(&bytes[1..]);
}

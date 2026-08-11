//! ADS-B declarations that need state across position and status messages.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::freshness::FieldQuality;

#[cfg(test)]
mod tests;

/// ADS-B protocol version whose integrity encoding is supported.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdsbVersion {
    /// RTCA DO-260A, ADS-B version one.
    Do260A,
    /// RTCA DO-260B, ADS-B version two.
    Do260B,
}

/// Integrity declarations from one ADS-B operational-status message.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdsbIntegrityDeclaration {
    version: AdsbVersion,
    nic_supplement_a_or_legacy: Option<bool>,
    nic_supplement_c: Option<bool>,
    nac_p: Option<u8>,
    sil: Option<u8>,
}

impl AdsbIntegrityDeclaration {
    /// Create a declaration after checking version-specific and range rules.
    pub fn new(
        version: AdsbVersion,
        nic_supplement_a_or_legacy: Option<bool>,
        nic_supplement_c: Option<bool>,
        nac_p: Option<u8>,
        sil: Option<u8>,
    ) -> Result<Self, InvalidAdsbIntegrityDeclaration> {
        if version == AdsbVersion::Do260A && nic_supplement_c.is_some() {
            return Err(InvalidAdsbIntegrityDeclaration::UnexpectedNicSupplementC);
        }
        if let Some(value) = nac_p
            && value > 11
        {
            return Err(InvalidAdsbIntegrityDeclaration::NacPRange { value });
        }
        if let Some(value) = sil
            && value > 3
        {
            return Err(InvalidAdsbIntegrityDeclaration::SilRange { value });
        }
        Ok(Self {
            version,
            nic_supplement_a_or_legacy,
            nic_supplement_c,
            nac_p,
            sil,
        })
    }

    /// Declared protocol version.
    #[must_use]
    pub const fn version(self) -> AdsbVersion {
        self.version
    }

    /// NIC supplement A, or the version-one legacy supplement.
    #[must_use]
    pub const fn nic_supplement_a_or_legacy(self) -> Option<bool> {
        self.nic_supplement_a_or_legacy
    }

    /// NIC supplement C for a version-two surface participant.
    #[must_use]
    pub const fn nic_supplement_c(self) -> Option<bool> {
        self.nic_supplement_c
    }

    /// Navigation accuracy category for position.
    #[must_use]
    pub const fn nac_p(self) -> Option<u8> {
        self.nac_p
    }

    /// Source integrity level.
    #[must_use]
    pub const fn sil(self) -> Option<u8> {
        self.sil
    }

    pub(crate) fn quality_for(self, position: AdsbPositionIntegrity) -> FieldQuality {
        FieldQuality::new(
            navigation_integrity_category(self, position),
            self.nac_p,
            None,
            self.sil,
        )
    }
}

/// Position-message class used by integrity derivation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdsbPositionKind {
    /// Airborne position message.
    Airborne,
    /// Surface position message.
    Surface,
}

/// Position type code and its message-local NIC supplement.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdsbPositionIntegrity {
    kind: AdsbPositionKind,
    type_code: u8,
    nic_supplement_b: Option<bool>,
}

impl AdsbPositionIntegrity {
    /// Create metadata for an airborne position message.
    pub fn airborne(
        type_code: u8,
        nic_supplement_b: Option<bool>,
    ) -> Result<Self, InvalidAdsbPositionIntegrity> {
        if !matches!(type_code, 9..=18 | 20..=22) {
            return Err(InvalidAdsbPositionIntegrity::TypeCode {
                kind: AdsbPositionKind::Airborne,
                value: type_code,
            });
        }
        Ok(Self {
            kind: AdsbPositionKind::Airborne,
            type_code,
            nic_supplement_b,
        })
    }

    /// Create metadata for a surface position message.
    pub fn surface(type_code: u8) -> Result<Self, InvalidAdsbPositionIntegrity> {
        if !matches!(type_code, 5..=8) {
            return Err(InvalidAdsbPositionIntegrity::TypeCode {
                kind: AdsbPositionKind::Surface,
                value: type_code,
            });
        }
        Ok(Self {
            kind: AdsbPositionKind::Surface,
            type_code,
            nic_supplement_b: None,
        })
    }

    /// Position-message class.
    #[must_use]
    pub const fn kind(self) -> AdsbPositionKind {
        self.kind
    }

    /// ADS-B position type code.
    #[must_use]
    pub const fn type_code(self) -> u8 {
        self.type_code
    }

    /// NIC supplement B from an airborne position message.
    #[must_use]
    pub const fn nic_supplement_b(self) -> Option<bool> {
        self.nic_supplement_b
    }
}

/// Invalid ADS-B operational-status integrity declaration.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum InvalidAdsbIntegrityDeclaration {
    /// Version one does not define NIC supplement C.
    #[error("DO-260A does not define NIC supplement C")]
    UnexpectedNicSupplementC,
    /// NACp exceeds the defined range.
    #[error("NACp {value} exceeds 11")]
    NacPRange {
        /// Rejected value.
        value: u8,
    },
    /// SIL exceeds the defined range.
    #[error("SIL {value} exceeds 3")]
    SilRange {
        /// Rejected value.
        value: u8,
    },
}

/// Invalid ADS-B position metadata.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum InvalidAdsbPositionIntegrity {
    /// Type code does not belong to the stated position class.
    #[error("type code {value} is not a {kind:?} position code")]
    TypeCode {
        /// Stated position class.
        kind: AdsbPositionKind,
        /// Rejected type code.
        value: u8,
    },
}

fn navigation_integrity_category(
    declaration: AdsbIntegrityDeclaration,
    position: AdsbPositionIntegrity,
) -> Option<u8> {
    match (declaration.version, position.kind) {
        (AdsbVersion::Do260A, AdsbPositionKind::Airborne) => {
            v1_airborne(position.type_code, declaration.nic_supplement_a_or_legacy)
        }
        (AdsbVersion::Do260A, AdsbPositionKind::Surface) => {
            v1_surface(position.type_code, declaration.nic_supplement_a_or_legacy)
        }
        (AdsbVersion::Do260B, AdsbPositionKind::Airborne) => v2_airborne(
            position.type_code,
            declaration.nic_supplement_a_or_legacy,
            position.nic_supplement_b,
        ),
        (AdsbVersion::Do260B, AdsbPositionKind::Surface) => v2_surface(
            position.type_code,
            declaration.nic_supplement_a_or_legacy,
            declaration.nic_supplement_c,
        ),
    }
}

fn v1_airborne(type_code: u8, supplement: Option<bool>) -> Option<u8> {
    match (type_code, supplement) {
        (9 | 20, Some(false)) => Some(11),
        (10 | 21, Some(false)) => Some(10),
        (11, Some(true)) => Some(9),
        (11, Some(false)) => Some(8),
        (12, Some(false)) => Some(7),
        (13, Some(_)) => Some(6),
        (14, Some(false)) => Some(5),
        (15, Some(false)) => Some(4),
        (16, Some(true)) => Some(3),
        (16, Some(false)) => Some(2),
        (17, Some(false)) => Some(1),
        (18 | 22, Some(false)) => Some(0),
        _ => None,
    }
}

fn v1_surface(type_code: u8, supplement: Option<bool>) -> Option<u8> {
    match (type_code, supplement) {
        (5, Some(false)) => Some(11),
        (6, Some(false)) => Some(10),
        (7, Some(true)) => Some(9),
        (7, Some(false)) => Some(8),
        (8, Some(false)) => Some(0),
        _ => None,
    }
}

fn v2_airborne(
    type_code: u8,
    supplement_a: Option<bool>,
    supplement_b: Option<bool>,
) -> Option<u8> {
    match (type_code, supplement_a, supplement_b) {
        (9 | 20, Some(false), Some(false)) => Some(11),
        (10 | 21, Some(false), Some(false)) => Some(10),
        (11, Some(true), Some(true)) => Some(9),
        (11, Some(false), Some(false)) => Some(8),
        (12, Some(false), Some(false)) => Some(7),
        (13, Some(false), Some(false | true)) | (13, Some(true), Some(true)) => Some(6),
        (14, Some(false), Some(false)) => Some(5),
        (15, Some(false), Some(false)) => Some(4),
        (16, Some(true), Some(true)) => Some(3),
        (16, Some(false), Some(false)) => Some(2),
        (17, Some(false), Some(false)) => Some(1),
        (18 | 22, Some(false), Some(false)) => Some(0),
        _ => None,
    }
}

fn v2_surface(type_code: u8, supplement_a: Option<bool>, supplement_c: Option<bool>) -> Option<u8> {
    match (type_code, supplement_a, supplement_c) {
        (5, Some(false), Some(false)) => Some(11),
        (6, Some(false), Some(false)) => Some(10),
        (7, Some(true), Some(false)) => Some(9),
        (7, Some(false), Some(false)) => Some(8),
        (8, Some(true), Some(true)) => Some(7),
        (8, Some(false), Some(true)) | (8, Some(true), Some(false)) => Some(6),
        (8, Some(false), Some(false)) => Some(0),
        _ => None,
    }
}

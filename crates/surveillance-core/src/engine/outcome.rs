//! Structured result of one successful ingest operation.

use crate::cpr::CprDecodeError;
use crate::fusion::FieldUpdate;
use crate::track::TrackId;

/// Result of offering one supplied field to fusion.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldDisposition {
    /// The observation did not supply this field.
    NotSupplied,
    /// Fusion kept the current field.
    Rejected,
    /// Fusion accepted the value and changed it.
    Changed,
    /// Fusion accepted new quality or provenance for the same value.
    MetadataChanged,
    /// Fusion accepted a newer time from the same source for the same value.
    Refreshed,
}

impl FieldDisposition {
    pub(crate) fn from_update(update: FieldUpdate) -> Self {
        match update {
            FieldUpdate::Rejected => Self::Rejected,
            FieldUpdate::Changed => Self::Changed,
            FieldUpdate::MetadataChanged => Self::MetadataChanged,
            FieldUpdate::Refreshed => Self::Refreshed,
        }
    }

    pub(crate) fn was_applied(self) -> bool {
        matches!(
            self,
            Self::Changed | Self::MetadataChanged | Self::Refreshed
        )
    }

    pub(crate) fn is_meaningful(self) -> bool {
        matches!(self, Self::Changed | Self::MetadataChanged)
    }

    /// Whether the observation supplied this field and fusion refused it.
    #[must_use]
    pub fn is_rejected(self) -> bool {
        self == Self::Rejected
    }
}

/// Why a supplied position did not enter fused state.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PositionRejection {
    /// CPR decoding or position plausibility failed.
    Decode(CprDecodeError),
    /// A current field won the fusion precedence decision.
    FusionPrecedence,
}

/// Result of applying one observation's position content.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PositionDisposition {
    /// The observation did not supply position content.
    NotSupplied,
    /// The position was accepted by fusion.
    Applied(FieldDisposition),
    /// The CPR frame is valid but needs an opposite parity frame.
    Pending(CprDecodeError),
    /// The position was rejected with a specific cause.
    Rejected(PositionRejection),
}

impl PositionDisposition {
    /// Whether supplied position content was rejected.
    #[must_use]
    pub fn is_rejected(self) -> bool {
        matches!(self, Self::Rejected(_))
    }
}

/// Outcomes for the non-position fields of one observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FieldOutcomes {
    /// Barometric pressure altitude outcome.
    pub pressure_altitude_ft: FieldDisposition,
    /// Geometric altitude outcome.
    pub geometric_altitude_ft: FieldDisposition,
    /// Velocity outcome.
    pub velocity: FieldDisposition,
    /// Callsign outcome.
    pub callsign: FieldDisposition,
    /// Squawk outcome.
    pub squawk: FieldDisposition,
    /// Air or ground state outcome.
    pub air_ground: FieldDisposition,
    /// Emergency state outcome.
    pub emergency: FieldDisposition,
    /// Emitter category outcome.
    pub emitter_category: FieldDisposition,
    /// ADS-B operational-status integrity declaration outcome.
    pub adsb_integrity: FieldDisposition,
    /// ADS-B position type-code metadata outcome.
    pub adsb_position_integrity: FieldDisposition,
}

impl FieldOutcomes {
    pub(crate) fn was_applied(self) -> bool {
        self.exported_values()
            .into_iter()
            .any(FieldDisposition::was_applied)
    }

    pub(crate) fn is_meaningful(self) -> bool {
        self.exported_values()
            .into_iter()
            .any(FieldDisposition::is_meaningful)
    }

    fn exported_values(self) -> [FieldDisposition; 8] {
        [
            self.pressure_altitude_ft,
            self.geometric_altitude_ft,
            self.velocity,
            self.callsign,
            self.squawk,
            self.air_ground,
            self.emergency,
            self.emitter_category,
        ]
    }

    fn rejected_count(self) -> u8 {
        self.exported_values()
            .into_iter()
            .chain([self.adsb_integrity, self.adsb_position_integrity])
            .filter(|outcome| outcome.is_rejected())
            .count() as u8
    }
}

/// Structured result of one observation that reached the engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IngestOutcome {
    /// Track selected before any association decision from this observation.
    pub track_id: TrackId,
    /// Whether this observation created that track.
    pub created: bool,
    /// Position outcome.
    pub position: PositionDisposition,
    /// Non-position field outcomes.
    pub fields: FieldOutcomes,
    /// Whether ingest emitted at least one state delta before association.
    pub delta_emitted: bool,
}

impl IngestOutcome {
    pub(crate) fn new(
        track_id: TrackId,
        created: bool,
        position: PositionDisposition,
        fields: FieldOutcomes,
        delta_emitted: bool,
    ) -> Self {
        Self {
            track_id,
            created,
            position,
            fields,
            delta_emitted,
        }
    }

    /// Number of supplied fields rejected during this ingest operation.
    #[must_use]
    pub fn rejected_field_count(self) -> u8 {
        self.fields.rejected_count() + u8::from(self.position.is_rejected())
    }
}

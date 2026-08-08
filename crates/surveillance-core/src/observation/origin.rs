//! What produced a surveillance fact.
//!
//! Origin is what the engine reasons about when two sources disagree: a
//! target's own transmission is better evidence about that target than a
//! ground system's estimate of it, whatever path either took to arrive. It is
//! deliberately not the same axis as delivery — a directly-transmitted ADS-B
//! report relayed by a provider is still ADS-B in origin, and a radar track
//! received on a local radio is still radar.

use serde::{Deserialize, Serialize};

use crate::source::Band;

/// What produced a surveillance observation.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservationOrigin {
    /// The target's own ADS-B transmission, received directly.
    AdsbDirect {
        /// Band the transmission was received on.
        band: Band,
    },
    /// A Mode S surveillance reply from the target.
    ///
    /// It comes from the target but carries neither position nor integrity
    /// indicators, so it is weaker evidence than the target's own ADS-B.
    ModeSReply,
    /// A ground rebroadcast of the target's own transmission from the other
    /// band.
    AdsR {
        /// Band the rebroadcast was received on.
        band: Band,
    },
    /// A ground-derived traffic information service report.
    Tisb {
        /// Band the uplink was received on.
        band: Band,
    },
    /// A radar-derived track.
    Radar,
    /// A position derived by multilateration of received transmissions.
    Mlat,
    /// A provider's fusion of upstream sources it did not itemize.
    ProviderFused,
    /// An installed panel's own fused track.
    PanelFused,
    /// A recorded or synthesized observation.
    Replay,
    /// The origin was not established.
    ///
    /// This is an explicit state rather than a default: a source that will
    /// not say what produced a report should not be read as if it had.
    Unknown,
}

impl ObservationOrigin {
    /// Band the observation was received on, when it was received by radio at
    /// all.
    #[must_use]
    pub fn band(&self) -> Option<Band> {
        match self {
            Self::AdsbDirect { band } | Self::AdsR { band } | Self::Tisb { band } => Some(*band),
            Self::ModeSReply => Some(Band::Adsb1090),
            Self::Radar
            | Self::Mlat
            | Self::ProviderFused
            | Self::PanelFused
            | Self::Replay
            | Self::Unknown => None,
        }
    }

    /// Whether the target itself produced the transmission this observation
    /// describes, even if something relayed it onward.
    #[must_use]
    pub fn is_from_the_target(&self) -> bool {
        matches!(
            self,
            Self::AdsbDirect { .. } | Self::ModeSReply | Self::AdsR { .. }
        )
    }

    /// Whether ground infrastructure derived or relayed this observation,
    /// which is what makes it a candidate for an ownship shadow.
    #[must_use]
    pub fn is_ground_derived(&self) -> bool {
        matches!(
            self,
            Self::AdsR { .. } | Self::Tisb { .. } | Self::Radar | Self::Mlat
        )
    }
}

//! Track identity, fused state, snapshots, and output deltas.

use crate::observation::{AirGroundState, EmergencyState};
use serde::{Deserialize, Serialize};

use alloc::vec::Vec;

use crate::association::CorrelationEvidence;
use crate::cpr::CprHistory;
use crate::freshness::TimedField;
use crate::identity::IdentitySet;
use crate::observation::{Callsign, Squawk, VelocityObservation, Wgs84Position};

#[cfg(test)]
mod tests;

/// Stable engine-assigned identity for one physical target.
///
/// The identity never changes across re-keying, promotion, or demotion;
/// consumers use it to correlate deltas over a track's whole life. Engines
/// assign identities with wrapping increments; constructing one directly is
/// for fixtures, replay, and tests.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct TrackId(u64);

impl TrackId {
    /// Wrap a numeric identity, e.g. one read back from a recording.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Numeric value for logging and correlation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Identity spaces that may reuse the same numeric address.
///
/// Equal numeric addresses in different namespaces are always distinct
/// tracks. ICAO-qualified identities form one namespace across links: direct
/// ADS-B, ADS-R, and TIS-B with an ICAO address associate on address
/// equality.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AddressNamespace {
    /// ICAO 24-bit aircraft address.
    Icao,
    /// Non-ICAO ADS-B address whose kind is not yet known.
    ///
    /// One protocol value covers anonymous, self-assigned, surface-vehicle,
    /// and fixed-obstacle addresses; only an emitter category tells them
    /// apart. Until one arrives the target is tracked but never correlated,
    /// so an unclassified vehicle can never be merged with an aircraft.
    AdsbNonIcao,
    /// Self-assigned or anonymized address.
    SelfAssigned,
    /// Ground-system track-file identifier.
    TisbTrackFile,
    /// Mode A code and track-file number relayed by TIS-B, packed as
    /// `mode_a_code << 12 | track_file_number`.
    ///
    /// This is a distinct identity space from [`Self::TisbTrackFile`]: a
    /// packed Mode A key and a service-generated 24-bit address can collide
    /// numerically while describing unrelated targets.
    TisbModeATrackFile,
    /// Surface vehicle address.
    SurfaceVehicle,
    /// Fixed ADS-B beacon address.
    FixedBeacon,
}

/// An over-the-air identity: numeric address plus its namespace.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct TrackKey {
    /// Identity space the address lives in.
    pub namespace: AddressNamespace,
    /// Numeric address within the namespace; 24 bits for ICAO.
    pub address: u32,
}

impl TrackKey {
    /// Create a key from a namespace and its numeric address.
    #[must_use]
    pub const fn new(namespace: AddressNamespace, address: u32) -> Self {
        Self { namespace, address }
    }
}

/// Lifecycle phase exported with every snapshot.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackPhase {
    /// Fresh observations are maintaining the track.
    Active,
    /// Defining fields lapsed their freshness windows; values are aging and
    /// the track awaits reacquisition or removal.
    Coasting,
}

/// Immutable consumer-facing state of one track.
///
/// A snapshot may be position-absent: a Mode S target can supply only
/// altitude, squawk, or identity, and position history begins when the first
/// valid position arrives. Every field keeps the provenance a detail view can
/// display. Snapshots serialize as part of the exported schema contract;
/// see [`crate::schema`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct TrackSnapshot {
    /// Stable engine-assigned identity.
    pub id: TrackId,
    /// Current primary over-the-air identity.
    pub key: TrackKey,
    /// All identities associated with the target.
    pub identities: IdentitySet,
    /// Lifecycle phase.
    pub phase: TrackPhase,
    /// Whether the track matches a caller-supplied ownship identity and is
    /// therefore a rebroadcast shadow of ownship, not an intruder. The
    /// engine labels; it never suppresses.
    pub ownship_shadow: bool,
    /// Fused WGS84 position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<TimedField<Wgs84Position>>,
    /// Fused barometric pressure altitude in feet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_altitude_ft: Option<TimedField<i32>>,
    /// Fused geometric (GNSS) altitude in feet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometric_altitude_ft: Option<TimedField<i32>>,
    /// Fused horizontal and vertical movement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<TimedField<VelocityObservation>>,
    /// Fused flight identification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<TimedField<Callsign>>,
    /// Fused Mode A transponder code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub squawk: Option<TimedField<Squawk>>,
    /// Fused air/ground state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air_ground: Option<TimedField<AirGroundState>>,
    /// Fused emergency or priority status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency: Option<TimedField<EmergencyState>>,
    /// Monotonic time the track was created.
    pub created_at_micros: u64,
    /// Monotonic time of the newest applied observation.
    pub last_observed_at_micros: u64,
}

impl TrackSnapshot {
    /// Create a position-absent snapshot holding identity alone; fused fields
    /// start absent and are set directly on the returned value.
    #[must_use]
    pub fn new(id: TrackId, key: TrackKey, created_at_micros: u64) -> Self {
        Self {
            id,
            key,
            identities: IdentitySet::new(key),
            phase: TrackPhase::Active,
            ownship_shadow: false,
            position: None,
            pressure_altitude_ft: None,
            geometric_altitude_ft: None,
            velocity: None,
            callsign: None,
            squawk: None,
            air_ground: None,
            emergency: None,
            created_at_micros,
            last_observed_at_micros: created_at_micros,
        }
    }
}

/// Latest fused surveillance state for one track: the exported snapshot plus
/// engine-internal working data such as CPR pairing history and correlation
/// evidence.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TrackState {
    /// Exported view of the fused state.
    pub snapshot: TrackSnapshot,
    pub(crate) cpr: CprHistory,
    pub(crate) correlation: Vec<CorrelationEvidence>,
    pub(crate) identity_activity: Vec<IdentityActivity>,
}

impl From<TrackSnapshot> for TrackState {
    fn from(snapshot: TrackSnapshot) -> Self {
        Self {
            snapshot,
            cpr: CprHistory::default(),
            correlation: Vec::new(),
            identity_activity: Vec::new(),
        }
    }
}

impl TrackState {
    /// Record that `key` was just observed, so a track carrying several
    /// identities knows which of them is still being fed.
    pub(crate) fn mark_identity_seen(&mut self, key: TrackKey, at_micros: u64) {
        for activity in &mut self.identity_activity {
            if activity.key == key {
                activity.last_observed_at_micros = activity.last_observed_at_micros.max(at_micros);
                return;
            }
        }
        self.identity_activity.push(IdentityActivity {
            key,
            last_observed_at_micros: at_micros,
        });
    }

    /// When `key` was last observed, if ever.
    pub(crate) fn identity_last_seen(&self, key: TrackKey) -> Option<u64> {
        self.identity_activity
            .iter()
            .find(|activity| activity.key == key)
            .map(|activity| activity.last_observed_at_micros)
    }
}

/// When one of a track's identities was last observed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct IdentityActivity {
    pub(crate) key: TrackKey,
    pub(crate) last_observed_at_micros: u64,
}

/// One engine output.
///
/// Re-keying is not removal: a track whose primary key changes appears as an
/// update carrying the same [`TrackId`], never as a removal plus a creation.
#[non_exhaustive]
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum TrackDelta {
    /// A track was created, possibly position-absent.
    Created(TrackSnapshot),
    /// A track's fused state meaningfully changed.
    Updated(TrackSnapshot),
    /// A track entered or continues coasting.
    Coasting(TrackSnapshot),
    /// A track was removed.
    Removed {
        /// Stable identity of the removed track.
        id: TrackId,
        /// Primary key at removal.
        key: TrackKey,
        /// Why the engine removed the track.
        reason: RemovalReason,
    },
}

impl TrackDelta {
    /// Stable identity of the track the delta describes.
    #[must_use]
    pub fn id(&self) -> TrackId {
        match self {
            Self::Created(snapshot) | Self::Updated(snapshot) | Self::Coasting(snapshot) => {
                snapshot.id
            }
            Self::Removed { id, .. } => *id,
        }
    }
}

/// Why the engine removed a track.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    /// Every field aged past the retention window.
    Expired,
    /// Correlation established that this track and another describe one
    /// aircraft. A consumer transfers what it was showing to `into` rather
    /// than dropping it: the aircraft did not disappear, its duplicate did.
    Merged {
        /// Track that absorbed this one and carries its history onward.
        into: TrackId,
    },
}

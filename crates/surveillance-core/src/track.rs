//! Track identity, fused state, snapshots, and output deltas.

use core::ops::Deref;

use crate::observation::{
    AdsbIntegrityDeclaration, AdsbPositionIntegrity, AirGroundState, EmergencyState,
    EmitterCategory,
};
use serde::{Deserialize, Serialize};

use alloc::vec::Vec;

use crate::association::CorrelationEvidence;
use crate::cpr::CprHistory;
use crate::freshness::TimedField;
use crate::identity::IdentitySet;
use crate::observation::{Callsign, Squawk, VelocityObservation, Wgs84Position};

#[cfg(test)]
mod tests;

/// Identity of one running producer instance.
///
/// The application supplies a new value each time it constructs an engine.
/// This keeps random-number and persistent identity policy outside the
/// runtime-neutral core. Consumers use the value only for equality.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProducerInstanceId(u64);

impl ProducerInstanceId {
    /// Create an identity from an application-assigned value.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the application-assigned value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Revision of one published state for one track.
///
/// A consumer compares revisions only when the producer identity and the
/// [`TrackId`] are equal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SnapshotRevision(u64);

impl SnapshotRevision {
    /// Create a revision from a serialized or stored value.
    #[must_use]
    pub const fn new(revision: u64) -> Self {
        Self(revision)
    }

    /// Get the numeric revision.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn checked_advance(self, count: u8) -> Option<Self> {
        let mut revision = self;
        for _ in 0..count {
            let next = revision.0.wrapping_add(1);
            if next == 0 {
                return None;
            }
            revision = Self(next);
        }
        Some(revision)
    }

    pub(crate) fn next(self) -> Option<Self> {
        self.checked_advance(1)
    }
}

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
    /// Fused source-neutral emitter category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emitter_category: Option<TimedField<EmitterCategory>>,
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
            emitter_category: None,
            created_at_micros,
            last_observed_at_micros: created_at_micros,
        }
    }
}

/// Retainable immutable view of one published track state.
///
/// The producer identity separates engines that can assign the same
/// [`TrackId`]. The revision orders published states of this track from one
/// producer. The handle owns its snapshot, so engine changes cannot change a
/// retained handle.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TrackSnapshotHandle {
    producer_instance_id: ProducerInstanceId,
    snapshot_revision: SnapshotRevision,
    snapshot: TrackSnapshot,
}

impl TrackSnapshotHandle {
    /// Create a handle from its envelope values and immutable state.
    #[must_use]
    pub const fn new(
        producer_instance_id: ProducerInstanceId,
        snapshot_revision: SnapshotRevision,
        snapshot: TrackSnapshot,
    ) -> Self {
        Self {
            producer_instance_id,
            snapshot_revision,
            snapshot,
        }
    }

    /// Identity of the engine that published this state.
    #[must_use]
    pub const fn producer_instance_id(&self) -> ProducerInstanceId {
        self.producer_instance_id
    }

    /// Revision assigned when this state was published.
    #[must_use]
    pub const fn snapshot_revision(&self) -> SnapshotRevision {
        self.snapshot_revision
    }

    /// Borrow the immutable track state.
    #[must_use]
    pub const fn snapshot(&self) -> &TrackSnapshot {
        &self.snapshot
    }

    /// Take the immutable track state from the handle.
    #[must_use]
    pub fn into_snapshot(self) -> TrackSnapshot {
        self.snapshot
    }
}

impl AsRef<TrackSnapshot> for TrackSnapshotHandle {
    fn as_ref(&self) -> &TrackSnapshot {
        &self.snapshot
    }
}

impl Deref for TrackSnapshotHandle {
    type Target = TrackSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.snapshot
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
    pub(crate) snapshot_revision: SnapshotRevision,
    pub(crate) cpr: CprHistory,
    pub(crate) correlation: Vec<CorrelationEvidence>,
    pub(crate) identity_activity: Vec<IdentityActivity>,
    pub(crate) adsb_integrity: Option<TimedField<AdsbIntegrityDeclaration>>,
    pub(crate) adsb_position_integrity: Option<TimedField<AdsbPositionIntegrity>>,
}

impl From<TrackSnapshot> for TrackState {
    fn from(snapshot: TrackSnapshot) -> Self {
        Self {
            snapshot,
            snapshot_revision: SnapshotRevision::new(0),
            cpr: CprHistory::default(),
            correlation: Vec::new(),
            identity_activity: Vec::new(),
            adsb_integrity: None,
            adsb_position_integrity: None,
        }
    }
}

impl TrackState {
    pub(crate) fn handle(&self, producer_instance_id: ProducerInstanceId) -> TrackSnapshotHandle {
        TrackSnapshotHandle::new(
            producer_instance_id,
            self.snapshot_revision,
            self.snapshot.clone(),
        )
    }

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
    Created(TrackSnapshotHandle),
    /// A track's fused state meaningfully changed.
    Updated(TrackSnapshotHandle),
    /// A track entered or continues coasting.
    Coasting(TrackSnapshotHandle),
    /// A track was removed.
    Removed {
        /// Identity of the engine that removed the track.
        producer_instance_id: ProducerInstanceId,
        /// Revision assigned to the removal.
        snapshot_revision: SnapshotRevision,
        /// Stable identity of the removed track.
        id: TrackId,
        /// Primary key at removal.
        key: TrackKey,
        /// Why the engine removed the track.
        reason: RemovalReason,
    },
}

impl TrackDelta {
    /// Identity of the engine that published the delta.
    #[must_use]
    pub fn producer_instance_id(&self) -> ProducerInstanceId {
        match self {
            Self::Created(handle) | Self::Updated(handle) | Self::Coasting(handle) => {
                handle.producer_instance_id()
            }
            Self::Removed {
                producer_instance_id,
                ..
            } => *producer_instance_id,
        }
    }

    /// Revision assigned to the published delta.
    #[must_use]
    pub fn snapshot_revision(&self) -> SnapshotRevision {
        match self {
            Self::Created(handle) | Self::Updated(handle) | Self::Coasting(handle) => {
                handle.snapshot_revision()
            }
            Self::Removed {
                snapshot_revision, ..
            } => *snapshot_revision,
        }
    }

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
